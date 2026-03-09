// Copyright © 2026 Jalapeno Labs

import { GoPlusCircle } from 'react-icons/go'
import { MdArchive, MdEditDocument, MdOutlineSettings, MdLogout } from 'react-icons/md'
import { FaRegTrashAlt, FaRegUserCircle, FaExclamationCircle } from 'react-icons/fa'
import { IoWarningOutline, IoEllipsisVerticalOutline } from 'react-icons/io5'
import { RxReset } from 'react-icons/rx'
import { FiEye, FiExternalLink, FiUpload } from 'react-icons/fi'
import { IoIosCheckmarkCircleOutline } from 'react-icons/io'
import { AiTwotoneCloseCircle } from 'react-icons/ai'

import CodexIcon from './icons/Codex.svg?react'
import DeepseekIcon from './icons/Deepseek.svg?react'

export {
  CodexIcon,
  DeepseekIcon,
}

export const CheckIcon = IoIosCheckmarkCircleOutline
export const WarningIcon = IoWarningOutline
export const DangerIcon = FaExclamationCircle
export const EditIcon = MdEditDocument
export const PlusIcon = GoPlusCircle
export const DeleteIcon = FaRegTrashAlt
export const ArchiveIcon = MdArchive
export const UserIcon = FaRegUserCircle
export const SettingsIcon = MdOutlineSettings
export const ExitIcon = MdLogout
export const EllipsisIcon = IoEllipsisVerticalOutline
export const ViewIcon = FiEye
export const ResetIcon = RxReset
export const CloseIcon = AiTwotoneCloseCircle
export const OpenIcon = FiExternalLink
export const UploadIcon = FiUpload
