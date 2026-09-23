import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import { emit } from '@tauri-apps/api/event';
import type { ConfigResult, ModelInfo, StatsResult } from '../../../desktop/src/bridge/types';

const config: ConfigResult = {
  theme: 'dark', ui_language: 'ru', language: 'ru', model: 'tiny', device: 'cpu',
  hotkey: 'Ctrl+Shift+Space', auto_paste: true, paste_trailing_space: false,
  paste_auto_submit: false, auto_start: false, sound_feedback: true, sound_volume: 0.35,
  duck_output_while_recording: false, duck_output_level: 0.2, trim_silence: true,
  history_retention_days: 30, history_max_entries: 1000, model_unload_after_minutes: 5,
  log_level: 'info', telemetry_enabled: false, debug_save_recordings: false,
  debug_overlay_diag: false, replacements_paused: false, recording_mode: 'toggle',
  microphone: null, typing_speed_cpm: 200, replacements: {}, replacement_rules: [],
  text_formatting: {
    enabled: true, remove_hallucinations: true, remove_fillers: true,
    remove_parasites: true, remove_duplicates: true, collapse_phrase_loops: true,
    clean_commas: true, normalize_spaces: true, correct_spelling: true, split_sentences: true,
    capitalize_sentences: true, final_punctuation: true, custom_parasite_words: [],
    disabled_parasite_words: [],
    custom_words: [], enabled_presets: [], dictionary_sets: [], dictionary_spellings: [],
  },
  ai_processing: {
    pipeline_mode: 'local', provider: 'openai', model: 'test-model',
    prompt_preset: 'default', spend_limit_usd: 5, system_prompt: '',
    profiles: [], key_slots: [],
  },
};
const models: ModelInfo[] = [
  { id: 'tiny', label: 'Whisper Tiny', size: '75 MB', ram: '400 MB', downloaded: true, selected: true, loaded: true, engine: 'whisper.cpp', family: 'Whisper' },
  { id: 'base', label: 'Whisper Base', size: '142 MB', ram: '500 MB', downloaded: false, selected: false, engine: 'whisper.cpp', family: 'Whisper' },
  { id: 'gigaam', label: 'GigaAM Test', size: '240 MB', ram: '1 GB', downloaded: true, selected: false, engine: 'sherpa-onnx', family: 'GigaAM', languages: ['ru'], cpu_only: true },
];
const stats: StatsResult = {
  total_transcriptions: 0, total_characters: 0, total_time_saved_seconds: 0,
  total_audio_seconds: 0, total_processing_seconds: 0, total_whisper_seconds: 0,
  total_format_seconds: 0, total_llm_seconds: 0, total_llm_attempts: 0,
  total_llm_used: 0, total_llm_fallbacks: 0, total_llm_input_tokens: 0,
  total_llm_output_tokens: 0, total_llm_tokens: 0, daily_history: [],
};

// Only the test runner injects this bundle; application entry points never import it.
export function install(seed: any = {}) {
  // Playwright accumulates init scripts, so a test that opens a second window
  // runs both of them in the new document. A second pass would re-wrap invoke
  // and double every subscription count, and its seed would lose to the first
  // one anyway, so the first harness keeps the page. conftest refuses the
  // second open_app outright; this is the guard behind that contract.
  if ((window as any).__sottoTest) return;
  const saved = sessionStorage.getItem('sotto-test-state');
  const state = saved ? JSON.parse(saved) : {
    config: { ...config, ...seed.config, text_formatting: { ...config.text_formatting, ...seed.config?.text_formatting }, ai_processing: { ...config.ai_processing, ...seed.config?.ai_processing } },
    whats_new: seed.whats_new ?? null,
    models: seed.models ?? models, stats: { ...stats, ...seed.stats }, history: seed.history ?? [],
    keys: seed.keys ?? {}, assessments: seed.assessments ?? [], runtime: { model_loaded: true, loaded_model: 'tiny', model: 'tiny', device: 'cpu', engine: 'whisper.cpp', recording: false, state: 'idle', // The tray styles itself for the platform whose popup commands the harness
    // stubs; another platform can be modelled through the `runtime` seed.
    os: 'windows', ...seed.runtime },
  };
  const calls: Array<{ command: string; args: any }> = [];
  const unknown: string[] = JSON.parse(sessionStorage.getItem('sotto-test-unknown') ?? '[]');
  const queues: Record<string, any[]> = structuredClone(seed.responses ?? {});
  const pending = new Map<string, { resolve: (value: any) => void; reject: (error: Error) => void }>();
  const subscriptions: Record<string, number> = {};
  let session = 0;
  const persist = () => sessionStorage.setItem('sotto-test-state', JSON.stringify(state));
  mockWindows(location.pathname.includes('overlay') ? 'overlay' : location.pathname.includes('tray') ? 'tray' : 'main');
  mockIPC(async (command, args: any = {}) => {
    calls.push({ command, args });
    const answer = queues[command]?.shift();
    if (answer) {
      if (answer.error) throw new Error(answer.error);
      if (answer.hold) return new Promise((resolve, reject) => pending.set(command, { resolve, reject }));
      return structuredClone(answer.result);
    }
    switch (command) {
      case 'get_whats_new': return state.whats_new ?? null;
      case 'dismiss_whats_new': state.whats_new = null; persist(); return null;
      case 'app_version': return { version: '0.0.5-test' };
      case 'check_accessibility': return true;
      case 'get_config': return structuredClone(state.config);
      case 'save_config':
        state.config = {
          ...state.config, ...args.patch,
          overlay: { ...state.config.overlay, ...args.patch.overlay },
          text_formatting: { ...state.config.text_formatting, ...args.patch.text_formatting },
          ai_processing: { ...state.config.ai_processing, ...args.patch.ai_processing },
        };
        state.models.forEach((m: ModelInfo) => { m.selected = m.id === state.config.model; });
        persist(); return structuredClone(state.config);
      case 'list_models': return structuredClone(state.models);
      case 'list_microphones': return [{ id: 'test-mic', name: 'Synthetic microphone' }];
      case 'get_runtime_status': return structuredClone(state.runtime);
      case 'get_stats': return structuredClone(state.stats);
      case 'list_history': return { entries: structuredClone(state.history), max_age_seconds: 2592000, max_entries: 1000 };
      case 'delete_history_entry': state.history = state.history.filter((e: any) => e.id !== args.id); persist(); return { deleted: true };
      case 'clear_history': { const count = state.history.length; state.history = []; persist(); return { deleted: count }; }
      case 'has_api_key': return state.keys[args.key_id] ?? { available: false, label: '', masked: '' };
      case 'save_api_key': state.keys[args.key_id] = { available: true, label: args.label, masked: 'test-***' }; persist(); return { saved: true, ...state.keys[args.key_id] };
      case 'delete_api_key': delete state.keys[args.key_id]; persist(); return { deleted: true };
      // Scores come from the seed: the harness does not reproduce the Rust
      // scoring, and a reset drops the model's row back to «no assessment».
      case 'model_assessments': return structuredClone(state.assessments);
      case 'reset_model_assessment':
        state.assessments = state.assessments.filter((a: { id: string }) => a.id !== args.id);
        persist(); return null;
      case 'dictionary_presets': return [['Test terms', ['Sotto', 'Playwright']]];
      // Synthetic sets, not a copy of the Rust constants: what the interface
      // must get right is showing whatever the backend sends, switching a set
      // on and an entry off. A second copy of the real lists would only drift.
      case 'parasite_sets': return [
        { id: 'ru', language: 'ru', words: ['ну', 'типа', 'короче'], default_on: true },
        { id: 'en', language: 'en', words: ['basically', 'like'], default_on: false },
      ];
      case 'analyze_dictionary': return { effective_count: 0, conflicts: [], unsupported_words: [] };
      // Processing outputs are fixtures, not a second implementation of the Rust engines.
      case 'preview_format': return { original: args.text, formatted: args.text };
      case 'preview_replacements': return { original: args.text, result: args.text, applied_count: 0, matched_rules: [] };
      case 'fetch_provider_models': return ['synthetic-model'];
      case 'get_output_contract': return 'Return only the processed text.';
      case 'check_update': return { available: false, current_version: '0.0.5-test' };
      case 'logs_size': return 1024;
      case 'clear_logs': return 0;
      case 'get_diagnostics': return 'Synthetic diagnostics';
      case 'get_public_diagnostics': return 'Sotto: 0.0.5-test\nOS: synthetic\nModel: tiny';
      case 'get_public_logs': return 'Public diagnostic log\n2026-09-14T12:00:00Z INFO app: [message omitted]';
      case 'save_public_logs': return true;
      case 'current_state': return seed.overlay_state ?? null;
      case 'start_recording': await emit('recording-started', ++session); return session;
      case 'stop_recording': await emit('recording-stopped', session); return session;
      case 'cancel_recording': await emit('whisper-cancelled', args.sessionId); return true;
      case 'pick_audio_file': return null;
      case 'set_model': return null;
      case 'delete_model': state.models.forEach((m: ModelInfo) => { if (m.id === args.model) m.downloaded = false; }); persist(); return null;
      case 'plugin:window|is_maximized': return false;
      case 'plugin:window|minimize': case 'plugin:window|toggle_maximize': case 'plugin:window|close':
      case 'plugin:window|start_dragging': case 'overlay_ready': case 'hide':
      case 'set_overlay_presentation': case 'hide_tray_popup': case 'focus_main_window': case 'open_url':
      case 'start_microphone_test': case 'stop_microphone_test': case 'set_microphone_test_monitor':
      case 'preview_sound_cue': case 'preview_output_duck': case 'open_diagnostics_folder':
      case 'suspend_hotkey': case 'resume_hotkey': return null;
      default:
        unknown.push(command);
        sessionStorage.setItem('sotto-test-unknown', JSON.stringify(unknown));
        throw new Error(`Unhandled test IPC: ${command}`);
    }
  }, { shouldMockEvents: true });
  // Observe subscription readiness rather than sleeping before sending events.
  const internals = (window as any).__TAURI_INTERNALS__;
  const invoke = internals.invoke;
  internals.invoke = async (command: string, args: any) => {
    const result = await invoke(command, args);
    if (command === 'plugin:event|listen') subscriptions[args.event] = (subscriptions[args.event] ?? 0) + 1;
    // An unlisten for an event that was never counted must not poison the
    // counter with NaN: emit() waits on `> 0` and would then never proceed.
    if (command === 'plugin:event|unlisten') subscriptions[args.event] = (subscriptions[args.event] ?? 0) - 1;
    return result;
  };
  (window as any).__sottoTest = {
    state, calls, unknown, subscriptions, emit,
    queue(command: string, answers: any[]) { (queues[command] ??= []).push(...answers); },
    pending: (command: string) => pending.has(command),
    settle(command: string, answer: any) {
      const request = pending.get(command);
      if (!request) throw new Error(`No pending command: ${command}`);
      pending.delete(command);
      if (answer.error) request.reject(new Error(answer.error)); else request.resolve(answer.result);
    },
  };
}
