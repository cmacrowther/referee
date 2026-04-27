import type { ComponentPropsWithoutRef } from "react"
import { Inter } from "next/font/google"
import { cn } from "@/lib/utils"

const inter = Inter({ subsets: ["latin"] })

const letters = ["R", "E", "F", "E", "R", "E", "E"] as const

const variantClasses = {
  nav: "gap-[0.05em] text-[18px]",
  inline: "gap-[0.025em] text-[0.92em] translate-y-[-0.02em]",
  display: "gap-[0.03em] text-[1em]",
} as const

type RefereeWordmarkProps = ComponentPropsWithoutRef<"span"> & {
  variant?: keyof typeof variantClasses
}

export function RefereeWordmark({
  className,
  variant = "inline",
  ...props
}: RefereeWordmarkProps) {
  return (
    <span
      className={cn(
        inter.className,
        "inline-flex items-baseline whitespace-nowrap font-extrabold uppercase leading-none text-white",
        variantClasses[variant],
        className
      )}
      {...props}
    >
      <span className="sr-only">REFEREE</span>
      {letters.map((letter, index) => (
        <span
          key={`${letter}-${index}`}
          aria-hidden="true"
          className={index === 2 ? "text-accent" : undefined}
        >
          {letter}
        </span>
      ))}
    </span>
  )
}
