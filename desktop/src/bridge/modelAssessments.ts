import { invoke } from "./invoke";

export type ModelAssessment = {
  id: string;
  compute: "cpu" | "gpu_unverified";
  speed: {
    score: number | null;
    source: "unknown" | "reference" | "personal";
    /** The reference came from another compute device or another language. */
    approximate: boolean;
    samples: number;
    median_ms: number | null;
    audio_min: number | null;
    audio_max: number | null;
    unstable: boolean;
    cold: boolean;
    reference: string | null;
  };
  memory: {
    score: number | null;
    status: "unknown" | "loaded" | "gpu_unknown" | "low" | "enough";
    required_bytes: number | null;
    available_bytes: number | null;
  };
  load_failed: boolean;
};

export function getModelAssessments(): Promise<ModelAssessment[]> {
  return invoke("model_assessments");
}
