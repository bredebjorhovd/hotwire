import type { ButtonHTMLAttributes, ReactNode } from "react";

export type ButtonVariant = "primary" | "secondary" | "ghost" | "icon";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  children: ReactNode;
}

/**
 * Primary control. A real `<button>`, so keyboard activation (Enter/Space) and
 * visible focus come for free (spec §17).
 */
export function Button({
  variant = "secondary",
  className,
  children,
  ...rest
}: ButtonProps) {
  const variantClass =
    variant === "primary"
      ? "btn--primary"
      : variant === "ghost"
        ? "btn--ghost"
        : variant === "icon"
          ? "btn--ghost btn--icon"
          : "";
  return (
    <button
      type="button"
      className={`btn ${variantClass}${className ? ` ${className}` : ""}`}
      {...rest}
    >
      {children}
    </button>
  );
}
