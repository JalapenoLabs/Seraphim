import { type ClassValue, clsx } from 'clsx'
import { twMerge } from 'tailwind-merge'

// The shadcn-svelte class merge helper: resolves conditional classes (clsx) then
// dedupes conflicting Tailwind utilities (tailwind-merge).
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

// Prop-shaping helpers the shadcn-svelte UI components rely on (they let a
// component accept an element ref and selectively drop Bits UI's `child`/
// `children` snippet props).
export type WithoutChild<Type> = Type extends { child?: unknown } ? Omit<Type, 'child'> : Type
export type WithoutChildren<Type> = Type extends { children?: unknown } ? Omit<Type, 'children'> : Type
export type WithoutChildrenOrChild<Type> = WithoutChildren<WithoutChild<Type>>
export type WithElementRef<Type, Element extends HTMLElement = HTMLElement> = Type & {
  ref?: Element | null
}
