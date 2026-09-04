"""A/B test old vs new system_prompt on stored history entries.

Reads history.json, picks entries where AI processing was used (so we have a
known `formatted_text` input + `text` reference output from the OLD prompt),
re-runs each entry through BOTH prompts against the currently-configured
provider, and writes results to scripts/ab_results.json.

Usage: python scripts/ab_prompts.py [--limit N] [--min-len N]
"""
from __future__ import annotations

import argparse
import json
import logging
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

from config import load_config, CONFIG_DIR  # noqa: E402
from ai_processor._step import ai_process_text_with_status  # noqa: E402
import transcription_history  # noqa: E402

logging.basicConfig(level=logging.WARNING, format="%(levelname)s %(name)s: %(message)s")
log = logging.getLogger("ab_prompts")
logging.getLogger("ai_processor").setLevel(logging.WARNING)

OLD_PROMPT = (
    "Ты помощник для обработки расшифровок диктовки.\n\n"
    "Исправь грамматику и пунктуацию, убери междометия и повторы, сохрани смысл и тон автора.\n\n"
    "Язык вывода: {{language}}.\n"
)


def run_one(text: str, base_ai_cfg: dict, prompt: str, *, max_retries: int = 4) -> tuple[str, dict]:
    cfg = dict(base_ai_cfg)
    cfg["system_prompt"] = prompt
    cfg["pipeline_mode"] = "hybrid"
    cfg["audio_duration_seconds"] = 60  # bypass min-duration gate
    cfg["llm_min_duration_seconds"] = 0
    backoff = 4.0
    for attempt in range(max_retries):
        t0 = time.time()
        out, status = ai_process_text_with_status(text, cfg)
        status["elapsed_seconds"] = round(time.time() - t0, 3)
        if status.get("skipped_reason") == "provider_quota_or_rate_limit":
            print(f"    rate-limited, sleeping {backoff:.0f}s (attempt {attempt + 1}/{max_retries})", flush=True)
            time.sleep(backoff)
            backoff *= 1.6
            continue
        return out, status
    return out, status


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--limit", type=int, default=0, help="Cap on entries (0 = all eligible)")
    ap.add_argument("--min-len", type=int, default=80, help="Skip entries shorter than this (chars)")
    ap.add_argument("--out", type=Path, default=Path(__file__).parent / "ab_results.json")
    args = ap.parse_args()

    full_cfg = load_config()
    ai_cfg = dict(full_cfg.get("ai_processing", {}))
    ai_cfg["language"] = full_cfg.get("language", "ru")
    new_prompt = ai_cfg.get("system_prompt", "")
    if not new_prompt:
        sys.exit("config has no system_prompt — abort")

    entries = transcription_history.list_entries()  # newest-first
    eligible = []
    for e in entries:
        ai = e.get("ai_processing") or {}
        if not ai.get("used"):
            continue
        src = e.get("formatted_text") or e.get("raw_text")
        if not src or len(src) < args.min_len:
            continue
        eligible.append(e)
    if args.limit > 0:
        eligible = eligible[: args.limit]

    print(f"Provider={ai_cfg.get('provider')} model={ai_cfg.get('model')} base_url={ai_cfg.get('base_url')}")
    print(f"Eligible entries: {len(eligible)}")
    print("-" * 80)

    results = []
    for i, entry in enumerate(eligible, 1):
        src = entry.get("formatted_text") or entry.get("raw_text") or ""
        old_stored = entry.get("text") or ""
        print(f"[{i}/{len(eligible)}] id={entry['id']} len_in={len(src)}", flush=True)
        try:
            out_old, st_old = run_one(src, ai_cfg, OLD_PROMPT)
        except Exception as ex:
            log.exception("old prompt run failed")
            out_old, st_old = "", {"error": repr(ex)}
        time.sleep(2.0)  # space out requests to avoid Cerebras RPM limit
        try:
            out_new, st_new = run_one(src, ai_cfg, new_prompt)
        except Exception as ex:
            log.exception("new prompt run failed")
            out_new, st_new = "", {"error": repr(ex)}
        time.sleep(2.0)
        results.append({
            "id": entry["id"],
            "input": src,
            "input_len": len(src),
            "stored_old_output": old_stored,
            "stored_old_len": len(old_stored),
            "rerun_old_output": out_old,
            "rerun_old_status": st_old,
            "rerun_old_len": len(out_old),
            "rerun_new_output": out_new,
            "rerun_new_status": st_new,
            "rerun_new_len": len(out_new),
            "ai_processing": entry.get("ai_processing"),
        })

    args.out.parent.mkdir(parents=True, exist_ok=True)
    with open(args.out, "w", encoding="utf-8") as f:
        json.dump(results, f, ensure_ascii=False, indent=2)
    print(f"Saved {len(results)} results to {args.out}")


if __name__ == "__main__":
    main()
