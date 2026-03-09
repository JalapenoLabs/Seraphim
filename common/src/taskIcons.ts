// Copyright © 2026 Jalapeno Labs

export const defaultTaskIconName = 'HiOutlineCommandLine' as const

export const taskIconLabelByName = {
  HiOutlineAcademicCap: 'learning, onboarding, education tasks',
  HiOutlineAdjustmentsHorizontal: 'fine-tuning settings and controls',
  HiOutlineArchiveBox: 'archiving and retention updates',
  HiOutlineArrowPath: 'retries, sync jobs, and refreshes',
  HiOutlineBars3: 'navigation and menu updates',
  HiOutlineBeaker: 'experiments, research, spikes',
  HiOutlineBell: 'alerts, reminders, and notifications',
  HiOutlineBolt: 'performance, speed, optimization',
  HiOutlineBookOpen: 'guides, docs, and documentation flows',
  HiOutlineBriefcase: 'business workflow and admin tasks',
  HiOutlineBuildingOffice2: 'organization and enterprise setup',
  HiOutlineBugAnt: 'bug fixes and debugging',
  HiOutlineCalendarDays: 'scheduling, calendar, and timeline work',
  HiOutlineChatBubbleBottomCenterText: 'chat, messaging, and conversations',
  HiOutlineChartBar: 'analytics, metrics, reporting',
  HiOutlineCheckCircle: 'verification, completion, and checks',
  HiOutlineCircleStack: 'database and persistence changes',
  HiOutlineClipboardDocumentList: 'planning, checklist, process work',
  HiOutlineCloudArrowUp: 'deployments, uploads, and publishing',
  HiOutlineCodeBracket: 'implementation and coding tasks',
  HiOutlineCodeBracketSquare: 'code review and structured code changes',
  HiOutlineCog6Tooth: 'configuration and setup changes',
  HiOutlineCommandLine: 'developer tooling and CLI tasks',
  HiOutlineComputerDesktop: 'desktop app and UI shell updates',
  HiOutlineCurrencyDollar: 'billing, pricing, and payments',
  HiOutlineCpuChip: 'infrastructure, systems, architecture',
  HiOutlineDocumentText: 'docs, writing, content updates',
  HiOutlineEnvelope: 'email, inbox, and outbound messaging',
  HiOutlineFlag: 'milestones, markers, and priorities',
  HiOutlineFunnel: 'filters, search criteria, and narrowing',
  HiOutlineGift: 'onboarding perks and promotional flows',
  HiOutlineGlobeAlt: 'api, networking, integration work',
  HiOutlineHashtag: 'tagging, taxonomy, and labeling',
  HiOutlineHome: 'dashboard and home experience updates',
  HiOutlineKey: 'authentication and access keys',
  HiOutlineLightBulb: 'ideas, brainstorming, and discovery',
  HiOutlineLink: 'linking resources and relationship mapping',
  HiOutlineLockClosed: 'permissions, auth, and data protection',
  HiOutlineMagnifyingGlass: 'investigation and root-cause analysis',
  HiOutlineMap: 'roadmap, planning, and dependency mapping',
  HiOutlinePaintBrush: 'styling, theming, and visual polish',
  HiOutlinePuzzlePiece: 'integration and plugin work',
  HiOutlineQrCode: 'qr flows and scannable data tasks',
  HiOutlineRocketLaunch: 'launch, release, delivery work',
  HiOutlineServerStack: 'backend services and server orchestration',
  HiOutlineShieldCheck: 'security and hardening tasks',
  HiOutlineShoppingCart: 'commerce and checkout experiences',
  HiOutlineSparkles: 'polish, UX improvements, refinement',
  HiOutlineStar: 'favorites, ratings, and featured content',
  HiOutlineTag: 'labels, tagging, and classification',
  HiOutlineTrophy: 'achievements, goals, and progress',
  HiOutlineUserGroup: 'team collaboration and member management',
  HiOutlineUsers: 'user management and account work',
  HiOutlineWrenchScrewdriver: 'maintenance and refactoring',
} as const

export type TaskIconName = keyof typeof taskIconLabelByName
export const taskIconNames = Object.keys(taskIconLabelByName) as TaskIconName[]

export function isTaskIconName(value: string | null | undefined): value is TaskIconName {
  if (!value?.trim()) {
    return false
  }

  return value in taskIconLabelByName
}

export function getValidTaskIconName(value: string | null | undefined): TaskIconName {
  if (!isTaskIconName(value)) {
    return defaultTaskIconName
  }

  return value
}
