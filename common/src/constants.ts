// Copyright © 2026 Jalapeno Labs

import type { LlmType } from '@prisma/client'

// Human UX
export const SEARCH_DEBOUNCE_MS = 400

// Project Configuration
export const CONFIG_FILE_NAME = 'seraphim.jsonc' as const

// Ports
export const API_PORT = 9900

// User Settings
export const USER_LANGUAGE_OPTIONS = [ 'auto', 'en-US' ] as const
export const USER_THEME_OPTIONS = [ 'system', 'dark', 'light' ] as const
export const DEFAULT_USER_LANGUAGE = 'auto' as const
export const DEFAULT_USER_THEME = 'system' as const
export const DEFAULT_VOICE_ENABLED = true as const
export const DEFAULT_VOICE_HOTKEY = 'Control+Num0' as const
export const USER_VOICE_PROVIDER_OPTIONS = [ 'NONE', 'NATIVE_CHROMIUM', 'LOCAL_VOSK', 'OPENAI_API_KEY' ] as const
export const DEFAULT_VOICE_PROVIDER = 'NONE' as const
export const DEFAULT_CUSTOM_AGENT_INSTRUCTIONS = '' as const
export const DEFAULT_CUSTOM_AGENTS_FILE = '' as const
export const DONE_SOUND_MIME_TYPES = [
  'audio/mpeg',
  'audio/wav',
  'audio/x-wav',
] as const
export const DONE_SOUND_FILE_EXTENSIONS = [ '.mp3', '.wav' ] as const

// Security
export const STANDARD_SECRET_WORDS = [
  'x-access-token',
  'gh_token',
  'github_token',
  'gitlab_token',
  'personal_access_token',
  'authorization',
  'proxy-authorization',
  'api-key',
  'api_key',
  'apikey',
  'access_token',
  'refresh_token',
  'private_key',
  'privatekey',
  'ssh_private_key',
  'secret_key',
  'secretkey',
  'session',
  'sessionid',
  'sid',
  'signing_secret',
] as const satisfies readonly string[]

// LLM
export const SUPPORTED_MODELS_BY_LLM = {
  OPENAI_API_KEY: [
    'gpt-5.2-codex',
    'gpt-5.1-codex-mini',
    'gpt-5.1-codex-max',
    'gpt-5.2',
    'gpt-5.1',
    'gpt-5.1-codex',
    'gpt-5-codex',
    'gpt-5-codex-mini',
    'gpt-5',
  ],
  OPENAI_LOGIN_TOKEN: [
    'gpt-5.2-codex',
    'gpt-5.1-codex-mini',
    'gpt-5.1-codex-max',
    'gpt-5.2',
    'gpt-5.1',
    'gpt-5.1-codex',
    'gpt-5-codex',
    'gpt-5-codex-mini',
    'gpt-5',
  ],
  DEEPSEEK_API_KEY: [
    'deepseek-chat',
    'deepseek-reasoner',
  ],
  CLAUDE_API_KEY: [
    'claude-sonnet-4-5',
    'claude-opus-4-1',
    'claude-3-7-sonnet-latest',
    'claude-3-5-haiku-latest',
  ],
} as const satisfies Partial<Record<LlmType, readonly string[]>>

// GitHub Auth
export const GITHUB_AUTH_PROVIDER_REQUIRED_SCOPES = [
  'repo',
  'read:user',
  'user:email',
] as const

// Issue tracking
export const DEFAULT_JIRA_CLOUD_BASE_URL = 'https://api.atlassian.com' as const
export const JIRA_CLOUD_REQUIRED_SCOPES = [
  'read:jira-work',
  'write:jira-work',
  'read:jira-user',
] as const

// Docker shenanigans
export const DEFAULT_DOCKER_BASE_IMAGE = 'mcr.microsoft.com/devcontainers/universal:5.1.4-noble' as const
export const DOCKER_USERNAME = 'primary' as const
export const DOCKER_WORKDIR = '/workspace' as const

export const DOCKER_USE_BUILDKIT = true as const

export const DOCKER_DEBIAN_PACKAGES = [
  'bash',
  'build-essential',
  'git',
  'gh',
  'curl',
  'openssh-client',
  'ca-certificates',
  'tar',
  'ripgrep',
  'time',
  'jq',
  'less',
  'findutils',
  'coreutils',
  'util-linux',
  'procps',
  'tzdata',
  'pkg-config',
  'libxi-dev',
  'libx11-dev',
  'libxext-dev',
  'libgl1-mesa-dev',
  'libglvnd-dev',
  'mesa-common-dev',
] as const

export const ACT_VERSION = '0.2.80' as const
export const ACT_SCRIPT_NAME = 'install-act.sh' as const

export const BACKUP_GITHUB_CLONE_SAMPLE_URL = 'https://github.com/github-samples/pets-workshop' as const

export const SETUP_SCRIPT_NAME = 'setup.sh' as const
export const VALIDATE_SCRIPT_NAME = 'validate.sh' as const
export const CODEX_WORKDIR = '/opt/.codex' as const

export const SETUP_SUCCESS_LINE = '======== SETUP SUCCESS ========' as const
export const SETUP_FAILURE_LINE = '======== SETUP FAILED  ========' as const

