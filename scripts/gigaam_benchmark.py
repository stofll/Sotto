"""Evaluate pinned upstream GigaAM candidates on prepared local speech."""

import argparse
import hashlib
import io
import json
import os
import platform
import tempfile
import time
from pathlib import Path

MODELS = {
    "gigaam-v4": (
        "https://huggingface.co/ai-sage/GigaAM-v4/tree/5b81216afa29c8b5a1a3e64c424c8416e18c1e5b",
        {
            "config.json": "e4fb0cf68ebe043cd456409352c311c8e946d3fc3a8dd3d7239925ba36020588",
            "modeling_gigaam.py": "dc0b2b56e98c55dddcce2ec6400ea8df3958883a700c153a869577bcf4f52db8",
            "model.safetensors": "f82cd5d3ef5158ac0318a21c207129da04040a191ca1c49078192826e3c01eed",
        },
    ),
    "gigaam-multilingual-v2": (
        "https://huggingface.co/ai-sage/GigaAM-Multilingual-v2/tree/c21483b5e185ff32f6fb2e2c801f92f4e7fbfee8",
        {
            "config.json": "a5d0b02874ba2956558a43a230b74faa71ae4dc491c26b68caf4f6c3cc862dda",
            "modeling_gigaam.py": "8d3f50834b60fe515d02232345314df083f9527910fb6e45fb5c4bd0f089c7a1",
            "model.safetensors": "1ea6b24faa308791b02e6916450a63b2a2ff401a7d890e08deaf34fc2acfe869",
            "raw_tokenizer.model": "c51fe228b747218891e3facdaeda12957065f4238452c377f38f76d3c2d4c423",
            "e2e_tokenizer.model": "3b049685e51a893350452f31613b47f4bd7640857f53facdf32266de82df3c0a",
        },
    ),
}
VAD_FILES = {
    "vad.safetensors": "fd8dfeba717992416d7f76689977c4ae9e9cc99737c1853ed625355b9f2bf692",
    "vad_config.json": "a2668614c2e0f8385115b6676c10c4cc02eb105bec4b4982582c56838c4b1141",
}


def cpu_name():
    if os.name == "nt":
        import winreg

        with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r"HARDWARE\DESCRIPTION\System\CentralProcessor\0") as key:
            return winreg.QueryValueEx(key, "ProcessorNameString")[0]
    return platform.processor()


def verify_model_files(weights, artifacts):
    for name, checksum in artifacts.items():
        with (weights / name).open("rb") as artifact:
            if hashlib.file_digest(artifact, "sha256").hexdigest() != checksum:
                raise ValueError(f"GigaAM artifact checksum mismatch for {name}")


def run(args):
    import soundfile
    import torch
    from transformers import AutoModel

    dataset, weights = args.dataset.resolve(), args.weights.resolve()
    manifest = args.input.read_bytes()
    rows = [json.loads(line) for line in manifest.splitlines() if line.strip()]
    if not rows or len({row["id"] for row in rows}) != len(rows):
        raise ValueError("Provide a nonempty manifest with unique IDs")
    for row in rows:
        if row["split"] != args.split or row["language"] not in {"ru", "en"} or not row["audio"]:
            raise ValueError("Use prepared RU/EN audio from the selected split")

    threads = min(os.cpu_count() or 1, 8)
    torch.set_num_threads(threads)
    torch.set_num_interop_threads(1)
    repository, model_files = MODELS[args.model]
    artifacts = model_files | VAD_FILES
    started = time.perf_counter()
    verify_model_files(weights, artifacts)
    model = AutoModel.from_pretrained(
        weights, trust_remote_code=True, local_files_only=True, use_safetensors=True,
        device="cpu", fp16_encoder=False,
    ).eval()
    load_ms = (time.perf_counter() - started) * 1000

    @torch.inference_mode()
    def infer(samples, language):
        started = time.perf_counter()
        waveform = torch.from_numpy(samples).unsqueeze(0)
        encoded, lengths = model(waveform, torch.tensor([len(samples)]), language=language)
        text = model.model.decode(encoded, lengths)["e2e"][0]
        return {"text": text, "elapsed_ms": (time.perf_counter() - started) * 1000}

    with args.output.open("x", encoding="utf-8", newline="\n") as output:
        def write(row):
            output.write(json.dumps(row, ensure_ascii=False) + "\n")
            output.flush()

        write({
            "type": "run", "schema": 1, "records": len(rows),
            "input_sha256": hashlib.sha256(manifest).hexdigest(),
            "runner_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            "options": {"--mode": "stt", "--split": args.split, "--model": args.model},
            "hints_mode": "empty", "compute": "cpu", "threads": threads,
            "cpu": cpu_name(), "os": platform.system().lower(),
            "model_source": repository, "artifacts_sha256": artifacts,
            "pytorch": torch.__version__, "head": "e2e",
        })
        write({"type": "load", "load_ms": load_ms, "includes_weight_verification": True})
        for index, row in enumerate(rows):
            audio = row["audio"]
            path = (dataset / audio["path"]).resolve()
            if not path.is_relative_to(dataset):
                raise ValueError("Audio escapes dataset")
            audio_bytes = path.read_bytes()
            if hashlib.sha256(audio_bytes).hexdigest() != audio["sha256"]:
                raise ValueError("Audio checksum mismatch")
            with soundfile.SoundFile(io.BytesIO(audio_bytes)) as wav:
                if (wav.format, wav.subtype, wav.channels, wav.samplerate) != ("WAV", "PCM_16", 1, 16000):
                    raise ValueError("Expected PCM16 mono WAV at 16 kHz")
                if not 0 < wav.frames <= 25 * wav.samplerate:
                    raise ValueError("Use nonempty clips up to 25 seconds for a single-pass comparison")
                samples = wav.read(dtype="float32")
            warmup = index == 0
            result = infer(samples, row["language"])
            write({
                "type": "record", "id": row["id"], "source": row["source"],
                "language": row["language"], "split": row["split"], "input": "",
                "audio_seconds": audio["duration_seconds"], "audio_sha256": audio["sha256"],
                "first_after_load": warmup, "repeat": infer(samples, row["language"]) if warmup else None,
                **result,
            })
            if index % 20 == 0:
                print(f"Evaluation saved {index + 1}/{len(rows)} records", flush=True)
        write({"type": "complete", "records": len(rows)})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", choices=MODELS, required=True)
    for option in ("--dataset", "--input", "--weights", "--output"):
        parser.add_argument(option, type=Path, required=True)
    parser.add_argument("--split", choices=("development", "holdout"), required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="sotto-gigaam-") as cache:
        os.environ.update(HF_HOME=cache, HF_MODULES_CACHE=cache, HF_HUB_OFFLINE="1", HF_HUB_DISABLE_TELEMETRY="1")
        run(args)


if __name__ == "__main__":
    main()
