/**
 * Keep the persisted model id and the engine's loaded model in lockstep.
 *
 * Loading is deliberately attempted first. A failed load (for example when a
 * model has not been downloaded yet) must not make the config claim that the
 * new engine is active. If saving itself fails after a successful load, put the
 * previous engine back before surfacing the original error.
 */
export async function loadThenPersistModel(
  modelId: string,
  previousModel: string | undefined,
  load: (modelId: string) => Promise<unknown>,
  persist: (patch: { model: string }) => Promise<unknown>,
): Promise<void> {
  await load(modelId);
  try {
    const result = await persist({ model: modelId });
    if (result === null) throw new Error("MODEL_CONFIG_SAVE_FAILED");
  } catch (error) {
    if (previousModel && previousModel !== modelId) {
      try {
        await load(previousModel);
      } catch {
        // Preserve the save error. The backend's runtime status remains the
        // source of truth if restoring the previous engine also fails.
      }
    }
    throw error;
  }
}
