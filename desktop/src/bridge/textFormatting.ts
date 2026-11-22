import { invoke } from "./invoke";
import type { ReplacementRule, ReplacementRuleValidationError } from "./types";

/** Uses the Rust regex parser that applies the rules during dictation. */
export function validateReplacementRules(rules: ReplacementRule[]): Promise<ReplacementRuleValidationError[]> {
  return invoke("validate_replacement_rules", { rules });
}
