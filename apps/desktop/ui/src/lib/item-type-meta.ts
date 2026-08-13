import {
  BookOpenText,
  Code2,
  FileQuestion,
  MessageSquareText,
  Wrench,
  type LucideIcon,
} from 'lucide-react'
import type { ItemType } from './api/types'

export interface ItemTypeMeta {
  label: string
  icon: LucideIcon
  chipClass: string
  badgeClass: string
}

const itemTypeMeta: Record<ItemType, ItemTypeMeta> = {
  knowledge: {
    label: '知识',
    icon: BookOpenText,
    chipClass: 'border-brand-200 bg-brand-100 text-brand-800',
    badgeClass: 'bg-brand-100 text-brand-800 border-brand-200',
  },
  skill: {
    label: '技能',
    icon: Wrench,
    chipClass: 'border-amber-200 bg-amber-100 text-amber-800',
    badgeClass: 'bg-amber-100 text-amber-800 border-amber-200',
  },
  snippet: {
    label: '片段',
    icon: Code2,
    chipClass: 'border-slate-200 bg-slate-100 text-slate-700',
    badgeClass: 'bg-slate-100 text-slate-700 border-slate-200',
  },
  observation: {
    label: '观察',
    icon: MessageSquareText,
    chipClass: 'border-violet-200 bg-violet-100 text-violet-800',
    badgeClass: 'bg-violet-100 text-violet-800 border-violet-200',
  },
}

const unknownItemTypeMeta: ItemTypeMeta = {
  label: '未知类型',
  icon: FileQuestion,
  chipClass: 'border-slate-200 bg-slate-50 text-slate-600',
  badgeClass: 'bg-slate-50 text-slate-600 border-slate-200',
}

export function getItemTypeMeta(itemType: string): ItemTypeMeta {
  if (!Object.prototype.hasOwnProperty.call(itemTypeMeta, itemType)) {
    return unknownItemTypeMeta
  }
  return itemTypeMeta[itemType as ItemType]
}
