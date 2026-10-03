"""Read public catalog metadata from Rust for README captures, without app data.

Only the literal manifest syntax used in model.rs is supported. A source-format
change fails the capture instead of silently falling back to a partial catalog.
"""

import hashlib
import json
import re
from pathlib import Path

SOURCE_DIR = Path(__file__).resolve().parents[2] / "desktop/src-tauri/src"


def array(source, name):
    match = re.search(rf"\bconst {name}: [^=]+?= &\[(.*?)\];", source, re.DOTALL)
    assert match, f"Cannot read Rust catalog array {name}"
    return match[1]


def entries(source, name, kind):
    body = array(source, name)
    blocks = re.findall(rf"\b{kind} \{{(.*?)\n    \}}", body, re.DOTALL)
    assert blocks and len(blocks) == body.count(f"{kind} {{"), name
    return [
        dict(re.findall(r"^\s+(\w+):\s*([^\n]+),$", block, re.MULTILINE))
        for block in blocks
    ]


def text(value):
    return json.loads(value)


def catalog():
    source = (SOURCE_DIR / "model.rs").read_text(encoding="utf-8")
    whispers = entries(source, "MODELS", "ModelDefinition")
    bundles = entries(source, "BUNDLE_MODEL_MANIFEST", "BundleModelManifestEntry")
    artifacts = entries(source, "MODEL_MANIFEST", "ModelManifestEntry")
    whisper_hashes = {text(row["public_id"]): text(row["sha256"]) for row in artifacts}
    assert {text(row["id"]) for row in whispers} == set(whisper_hashes)
    tags = re.findall(r'"([^"]+)"', array(source, "QUANTIZATION_TAGS"))
    models, revisions = [], {}
    for bundle, rows in ((False, whispers), (True, bundles)):
        for row in rows:
            model_id = text(row["public_id" if bundle else "id"])
            languages = row["languages"]
            if languages == "None":
                languages = None
            elif languages.startswith("Some(&["):
                languages = text(languages[len("Some(&") : -1])
            else:
                constant = re.fullmatch(r"Some\((\w+)\)", languages)
                assert constant, languages
                languages = re.findall(r'"([^"]+)"', array(source, constant[1]))
            if bundle:
                files = array(source, row["artifacts"])
                filenames = re.findall(r'file_name: "([^"]+)"', files)
                hashes = re.findall(r'sha256: "([^"]+)"', files)
                assert filenames and len(filenames) == len(hashes)
                revisions[model_id] = hashlib.sha256(
                    "".join(hashes).encode()
                ).hexdigest()
                quantization = next(
                    (tag for file in filenames for tag in tags if tag in file), None
                )
            else:
                revisions[model_id] = whisper_hashes[model_id]
                quantization = next(
                    (tag for tag in tags if tag in text(row["file_stem"])), "f16"
                )
            ram_mib = int(row["ram_mib"].replace("_", ""))
            models.append(
                {
                    "id": model_id,
                    "label": text(row["label"]),
                    "family": text(row["family"]) if bundle else "Whisper",
                    "size": text(row["size"]),
                    "ram": f"~{ram_mib / 1024:.1f} GB",
                    "ram_bytes": ram_mib * 1024 * 1024,
                    "languages": languages,
                    "engine": "sherpa-onnx" if bundle else "whisper.cpp",
                    "compute_backend": "CPU" if bundle else "CPU/GPU",
                    "cpu_only": bundle,
                    "streaming": row.get("engine")
                    == "ModelEngine::SherpaStreamingTransducer",
                    "quantization": quantization,
                    "recommended": text(row["recommended"]),
                    # Only installation and selection are demonstration state.
                    "downloaded": model_id in ("gigaam-v3", "nemotron-streaming"),
                    "selected": model_id == "nemotron-streaming",
                    "loaded": model_id == "nemotron-streaming",
                    "local": False,
                }
            )
    assert len({model["id"] for model in models}) == len(models)
    return models, revisions


def assessments(models, revisions):
    references = json.loads((SOURCE_DIR / "model_reference.json").read_text())
    performance = (SOURCE_DIR / "model_performance.rs").read_text(encoding="utf-8")
    method = re.search(r'const METHOD: &str = "([^"]+)"', performance)[1]
    result = []
    for model in models:
        reference = next(
            (
                row
                for row in references
                if row["model_id"] == model["id"]
                and row["revision"] == revisions[model["id"]]
                and row["compute"] == "cpu"
                and row["language"] == "auto"
                and row["method"] == method
            ),
            None,
        )
        result.append(
            {
                "id": model["id"],
                "compute": "cpu",
                # Same bounded scale as model_performance::speed_score.
                "speed": {
                    "score": 1 / (1 + reference["rtf"]) if reference else None,
                    "source": "reference" if reference else "unknown",
                },
                "memory": {
                    "score": None,
                    "status": "unknown",
                    "required_bytes": model["ram_bytes"],
                    "available_bytes": None,
                },
            }
        )
    return result
