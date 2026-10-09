/** What the capability scene lists, in the order a new user meets it. */
export const capabilities = ['voice', 'cleanup', 'models', 'profile', 'history', 'file'] as const;
export type CapabilityId = typeof capabilities[number];

/** Catalog entries the models card shows; sizes and traits come from `models` in product.ts. */
export const capabilityModels = ['GigaAM v3', 'Nemotron 3.5', 'Whisper turbo'];

/** Formats the file panel accepts, mirroring `AUDIO_EXTENSIONS` in `desktop/src/pages/AiPage.tsx`. */
export const audioExtensions = ['wav', 'mp3', 'm4a', 'mp4', 'ogg', 'oga', 'opus', 'flac'];
