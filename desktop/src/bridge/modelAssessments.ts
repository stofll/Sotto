import { invoke } from "./invoke";

export type ModelAssessment = {
  id: string;
  compute: "cpu" | "gpu_unverified";
  /** A comparative CPU benchmark under the configured language, not a
   * prediction for this machine. The card draws three levels from `score`. */
  speed: {
    score: number | null;
    source: "unknown" | "reference";
  };
  memory: {
    score: number | null;
    status: "unknown" | "loaded" | "gpu_unknown" | "low" | "enough";
    required_bytes: number | null;
    available_bytes: number | null;
  };
  download?: { required_bytes: number | null; available_bytes: number | null; insufficient: boolean };
};

export function getModelAssessments(): Promise<ModelAssessment[]> {
  return invoke("model_assessments");
}
