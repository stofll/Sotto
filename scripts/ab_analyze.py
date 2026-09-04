"""Analyze ab_results.json: length deltas, refusal detection, divergence metric."""
from __future__ import annotations

import json
import re
import sys
from difflib import SequenceMatcher
from pathlib import Path

ROOT = Path(__file__).resolve().parent
RESULTS = ROOT / "ab_results.json"

REFUSAL_PATTERNS = [
    r"извин",
    r"не могу помочь",
    r"я не могу",
    r"i (cannot|can't|won't)",
    r"as an ai",
    r"я ии",
]
REFUSAL_RE = re.compile("|".join(REFUSAL_PATTERNS), re.IGNORECASE)

# Markers that suggest the input itself is a command/instruction.
COMMAND_HINTS = re.compile(
    r"\b(сделай|добавь|удали|поправь|перепиши|объясни|реализуй|напиши|почему|зачем|как)\b",
    re.IGNORECASE,
)


def similarity(a: str, b: str) -> float:
    return SequenceMatcher(None, a, b).ratio()


def main():
    with open(RESULTS, encoding="utf-8") as f:
        rows = json.load(f)

    print(f"Loaded {len(rows)} A/B rows\n")
    print(f"{'id':>14} {'in':>5} {'old':>5} {'new':>5} {'oldΔ':>6} {'newΔ':>6} {'sim(old,new)':>12} {'cmd':>4} {'refuseO':>8} {'refuseN':>8}")
    print("-" * 90)

    stats = {"refusal_old": 0, "refusal_new": 0, "command_inputs": 0, "len_old_short": 0, "len_new_short": 0}
    sims = []
    new_close_to_input = 0
    old_close_to_input = 0
    new_close_to_old = 0

    for r in rows:
        src = r["input"]
        old_out = r["rerun_old_output"] or ""
        new_out = r["rerun_new_output"] or ""
        cmd = bool(COMMAND_HINTS.search(src))
        refuse_o = bool(REFUSAL_RE.search(old_out))
        refuse_n = bool(REFUSAL_RE.search(new_out))
        sim_on = similarity(old_out, new_out)
        sim_oi = similarity(old_out, src)
        sim_ni = similarity(new_out, src)
        sims.append(sim_on)
        if sim_oi > 0.97:
            old_close_to_input += 1
        if sim_ni > 0.97:
            new_close_to_input += 1
        if sim_on > 0.97:
            new_close_to_old += 1
        if cmd:
            stats["command_inputs"] += 1
        if refuse_o:
            stats["refusal_old"] += 1
        if refuse_n:
            stats["refusal_new"] += 1
        # "drastically shorter" = lost > 25% of content
        if len(old_out) < len(src) * 0.75:
            stats["len_old_short"] += 1
        if len(new_out) < len(src) * 0.75:
            stats["len_new_short"] += 1

        old_d = len(old_out) - len(src)
        new_d = len(new_out) - len(src)
        print(f"{r['id']:>14} {len(src):>5} {len(old_out):>5} {len(new_out):>5} {old_d:>+6} {new_d:>+6} {sim_on:>12.3f} {'Y' if cmd else '':>4} {'Y' if refuse_o else '':>8} {'Y' if refuse_n else '':>8}")

    print("-" * 90)
    print(f"refusals: old={stats['refusal_old']} new={stats['refusal_new']}")
    print(f"inputs that look like commands/questions: {stats['command_inputs']}")
    print(f"outputs >25% shorter than input: old={stats['len_old_short']} new={stats['len_new_short']}")
    print(f"outputs ≈identical to input (>0.97 sim): old={old_close_to_input} new={new_close_to_input}")
    print(f"new ≈identical to old (>0.97 sim): {new_close_to_old}")
    print(f"avg similarity(old, new) = {sum(sims) / len(sims):.3f}")
    print(f"min sim = {min(sims):.3f}, max sim = {max(sims):.3f}")

    print("\n=== Top 5 most divergent old vs new (lowest similarity) ===")
    rows_sorted = sorted(rows, key=lambda r: similarity(r["rerun_old_output"], r["rerun_new_output"]))
    for r in rows_sorted[:5]:
        sim = similarity(r["rerun_old_output"], r["rerun_new_output"])
        print(f"\n--- id={r['id']} sim={sim:.3f} len_in={r['input_len']} ---")
        print(f"INPUT: {r['input'][:400]}{'…' if len(r['input']) > 400 else ''}")
        print(f"OLD:   {r['rerun_old_output'][:400]}{'…' if len(r['rerun_old_output']) > 400 else ''}")
        print(f"NEW:   {r['rerun_new_output'][:400]}{'…' if len(r['rerun_new_output']) > 400 else ''}")


if __name__ == "__main__":
    main()
