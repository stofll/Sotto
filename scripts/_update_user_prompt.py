"""One-shot: rewrite system_prompt in user's saved config.json to the new default."""
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from config import CONFIG_FILE, DEFAULT_CONFIG, _atomic_write_json  # type: ignore

new_prompt = DEFAULT_CONFIG["ai_processing"]["system_prompt"]

with open(CONFIG_FILE, "r", encoding="utf-8") as f:
    cfg = json.load(f)

cfg.setdefault("ai_processing", {})["system_prompt"] = new_prompt
_atomic_write_json(CONFIG_FILE, cfg, ensure_ascii=False)
print(f"Updated system_prompt in {CONFIG_FILE} (length={len(new_prompt)})")
