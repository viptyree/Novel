import { invoke } from "@tauri-apps/api/core";

export interface CommandResult {
  success: boolean;
  log: string;
}

export type LlmProvider = "api" | "claude_code" | "codex";

export interface Settings {
  project_dir: string;
  llm_base_url: string;
  llm_api_key: string;
  llm_model: string;
  disable_thinking?: boolean;
  // LLM 路线：api(OpenAI 兼容) | claude_code(Claude Code 订阅) | codex(Codex/ChatGPT 订阅)
  llm_provider?: LlmProvider;
}

export type GenerationPreset = "general" | "compact" | "benchmark";
export interface TaskPrompt { system: string; user_template: string; }
export interface PromptSettings extends TaskPrompt {
  generation_preset: GenerationPreset;
  compact_generation: TaskPrompt;
  benchmark_generation: TaskPrompt;
  general_material_guidance: boolean;
  compact_material_guidance: boolean;
  benchmark_material_guidance: boolean;
  consistency: TaskPrompt; memory: TaskPrompt;
}
export interface PromptPair { system: string; user: string; }
export interface ContextOptions {
  generation_preset?: GenerationPreset | null;
  automatic: boolean; characters: string[]; chapters: string[]; events: string[];
  recent_chapters: number; max_events: number; input_budget: number;
  include_outline: boolean; outline_excerpt: string;
}
export const defaultContextOptions: ContextOptions = {
  automatic: true, characters: [], chapters: [], events: [], recent_chapters: 2,
  max_events: 6, input_budget: 8000, include_outline: false, outline_excerpt: "",
};
export interface ContextSource { kind: string; id: string; file: string; reason: string; included: boolean; characters: number; }
export interface ContextStats { characters: number; estimated_tokens: number; budget: number; over_budget: boolean; sources: ContextSource[]; warnings: string[]; }
export interface PromptPreview { prompts: PromptPair; source: string; stats: ContextStats; }
export interface ContextCandidate { id: string; label: string; chapter: number | null; }
export interface ContextCatalog { characters: ContextCandidate[]; chapters: ContextCandidate[]; events: ContextCandidate[]; source_directory: string; chapter_file: string; notes: string; }
export interface SourceSettings { directory: string; }
export interface ChapterDetails { id: string; title: string; path: string; }
export interface ProjectEntry {
  name: string; path: string; first_opened_at: string; last_opened_at: string;
  pinned: boolean; exists: boolean; current: boolean;
}
export interface OpenProjectOptions { path: string; create: boolean; name?: string; previousPath?: string; }


export type ReportKind = "generation" | "consistency" | "memory";

export const api = {
  listProjects: () => invoke<ProjectEntry[]>("list_projects"),
  openProject: (options: OpenProjectOptions) => invoke<string>("open_project", { ...options, name: options.name ?? null, previousPath: options.previousPath ?? null }),
  updateProjectEntry: (path: string, pinned: boolean | null, remove = false) =>
    invoke<void>("update_project_entry", { path, pinned, remove }),
  getPipelineRoot: () => invoke<string>("get_pipeline_root"),

  listChapters: () => invoke<string[]>("list_chapters"),
  listChapterDetails: () => invoke<ChapterDetails[]>("list_chapter_details"),
  generateChapterTitles: (projectRoot: string, chapterId: string, content: string, previousCandidates: string[]) =>
    invoke<string[]>("generate_chapter_titles", { projectRoot, chapterId, content, previousCandidates }),
  saveChapterTitle: (projectRoot: string, chapterId: string, title: string, expectedPath: string) =>
    invoke<ChapterDetails>("save_chapter_title", { projectRoot, chapterId, title, expectedPath }),
  readChapter: (chapterId: string) =>
    invoke<string>("read_chapter", { chapterId }),
  saveChapter: (chapterId: string, content: string, expected?: string) =>
    invoke<void>("save_chapter", { chapterId, content, expected: expected ?? null }),

  readReport: (chapterId: string, kind: ReportKind) =>
    invoke<string>("read_report", { chapterId, kind }),

  openUrl: (url: string) => invoke<void>("open_url", { url }),

  readMemoryFile: (rel: string) =>
    invoke<string>("read_memory_file", { rel }),
  saveMemoryFile: (rel: string, content: string, expected?: string) =>
    invoke<void>("save_memory_file", { rel, content, expected: expected ?? null }),

  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings, expected?: Settings) =>
    invoke<void>("save_settings", { settings, expected: expected ?? null }),

  contextCatalog: (chapterId: string) => invoke<ContextCatalog>("context_catalog", { chapterId }),
  getSourceSettings: () => invoke<SourceSettings>("get_source_settings"),
  saveSourceSettings: (settings: SourceSettings, expected?: string) => invoke<void>("save_source_settings", { settings, expected: expected ?? null }),
  getPromptSettings: () => invoke<PromptSettings>("get_prompt_settings"),
  defaultPromptSettings: () => invoke<PromptSettings>("default_prompt_settings"),
  savePromptSettings: (settings: PromptSettings, expected?: PromptSettings) => invoke<void>("save_prompt_settings", { settings, expected: expected ?? null }),
  previewPrompt: (chapterId: string, idea: string, targetWords: number, useContext: boolean, pov: string, narrative: string, contextOptions: ContextOptions) =>
    invoke<PromptPreview>("preview_generation_prompt", { chapterId, idea, targetWords, useContext, pov, narrative, contextOptions }),

  generateChapter: (
    chapterId: string,
    idea: string,
    targetWords: number,
    useContext: boolean,
    overwrite: boolean,
    pov: string,
    narrative: string,
    promptOverride: PromptPreview | undefined,
    contextOptions: ContextOptions
  ) =>
    invoke<CommandResult>("generate_chapter", {
      chapterId,
      idea,
      targetWords,
      useContext,
      overwrite,
      pov,
      narrative,
      promptOverride: promptOverride ?? null,
      contextOptions,
    }),

  checkConsistency: (chapterId: string) =>
    invoke<CommandResult>("check_consistency", { chapterId }),

  updateMemory: (chapterId: string) =>
    invoke<CommandResult>("update_memory", { chapterId }),

  resetChapter: (chapterId: string, includeMemory: boolean) =>
    invoke<CommandResult>("reset_chapter", { chapterId, includeMemory }),
};
