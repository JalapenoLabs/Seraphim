// Copyright © 2026 Jalapeno Labs

import type { IconType } from 'react-icons'
import type { Task } from '@common/types'

// User Interface
import {
  HiOutlineAcademicCap,
  HiOutlineAdjustmentsHorizontal,
  HiOutlineArchiveBox,
  HiOutlineArrowPath,
  HiOutlineBars3,
  HiOutlineBeaker,
  HiOutlineBell,
  HiOutlineBolt,
  HiOutlineBookOpen,
  HiOutlineBriefcase,
  HiOutlineBuildingOffice2,
  HiOutlineBugAnt,
  HiOutlineCalendarDays,
  HiOutlineChatBubbleBottomCenterText,
  HiOutlineChartBar,
  HiOutlineCheckCircle,
  HiOutlineCircleStack,
  HiOutlineClipboardDocumentList,
  HiOutlineCloudArrowUp,
  HiOutlineCodeBracket,
  HiOutlineCodeBracketSquare,
  HiOutlineCog6Tooth,
  HiOutlineCommandLine,
  HiOutlineComputerDesktop,
  HiOutlineCurrencyDollar,
  HiOutlineCpuChip,
  HiOutlineDocumentText,
  HiOutlineEnvelope,
  HiOutlineFlag,
  HiOutlineFunnel,
  HiOutlineGift,
  HiOutlineGlobeAlt,
  HiOutlineHashtag,
  HiOutlineHome,
  HiOutlineKey,
  HiOutlineLightBulb,
  HiOutlineLink,
  HiOutlineLockClosed,
  HiOutlineMagnifyingGlass,
  HiOutlineMap,
  HiOutlinePaintBrush,
  HiOutlinePuzzlePiece,
  HiOutlineQrCode,
  HiOutlineRocketLaunch,
  HiOutlineServerStack,
  HiOutlineShieldCheck,
  HiOutlineShoppingCart,
  HiOutlineSparkles,
  HiOutlineStar,
  HiOutlineTag,
  HiOutlineTrophy,
  HiOutlineUserGroup,
  HiOutlineUsers,
  HiOutlineWrenchScrewdriver,
} from 'react-icons/hi2'

// Misc
import { defaultTaskIconName, getValidTaskIconName, type TaskIconName } from '@common/taskIcons'

const iconByName: Record<TaskIconName, IconType> = {
  HiOutlineAcademicCap,
  HiOutlineAdjustmentsHorizontal,
  HiOutlineArchiveBox,
  HiOutlineArrowPath,
  HiOutlineBars3,
  HiOutlineBeaker,
  HiOutlineBell,
  HiOutlineBolt,
  HiOutlineBookOpen,
  HiOutlineBriefcase,
  HiOutlineBuildingOffice2,
  HiOutlineBugAnt,
  HiOutlineCalendarDays,
  HiOutlineChatBubbleBottomCenterText,
  HiOutlineChartBar,
  HiOutlineCheckCircle,
  HiOutlineCircleStack,
  HiOutlineClipboardDocumentList,
  HiOutlineCloudArrowUp,
  HiOutlineCodeBracket,
  HiOutlineCodeBracketSquare,
  HiOutlineCog6Tooth,
  HiOutlineCommandLine,
  HiOutlineComputerDesktop,
  HiOutlineCurrencyDollar,
  HiOutlineCpuChip,
  HiOutlineDocumentText,
  HiOutlineEnvelope,
  HiOutlineFlag,
  HiOutlineFunnel,
  HiOutlineGift,
  HiOutlineGlobeAlt,
  HiOutlineHashtag,
  HiOutlineHome,
  HiOutlineKey,
  HiOutlineLightBulb,
  HiOutlineLink,
  HiOutlineLockClosed,
  HiOutlineMagnifyingGlass,
  HiOutlineMap,
  HiOutlinePaintBrush,
  HiOutlinePuzzlePiece,
  HiOutlineQrCode,
  HiOutlineRocketLaunch,
  HiOutlineServerStack,
  HiOutlineShieldCheck,
  HiOutlineShoppingCart,
  HiOutlineSparkles,
  HiOutlineStar,
  HiOutlineTag,
  HiOutlineTrophy,
  HiOutlineUserGroup,
  HiOutlineUsers,
  HiOutlineWrenchScrewdriver,
}

type Props = {
  task: Task
  className?: string
  size?: number
}

export function TaskIcon(props: Props) {
  const iconName = getValidTaskIconName(props.task.icon)
  const Icon = iconByName[iconName] ?? iconByName[defaultTaskIconName]

  return <Icon className={props.className} size={props.size} />
}
