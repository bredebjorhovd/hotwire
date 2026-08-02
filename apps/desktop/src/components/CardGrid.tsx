import { useRef, type KeyboardEvent, type ReactNode } from "react";

export interface CardOption {
  id: string;
  title: string;
  description?: string;
  badge?: string;
  recommended?: boolean;
  render?: () => ReactNode;
}

export interface CardGridProps {
  options: CardOption[];
  selectedId?: string | null;
  onSelect: (id: string) => void;
  ariaLabel?: string;
  className?: string;
}

/**
 * A keyboard-navigable card list (spec §17). Arrow keys move selection and
 * focus; Enter/Space activate through the native button.
 */
export function CardGrid({
  options,
  selectedId,
  onSelect,
  ariaLabel,
  className,
}: CardGridProps) {
  const refs = useRef<Array<HTMLButtonElement | null>>([]);

  const move = (from: number, delta: number) => {
    const next = Math.max(0, Math.min(options.length - 1, from + delta));
    refs.current[next]?.focus();
  };

  const handleKeyDown = (
    index: number,
    event: KeyboardEvent<HTMLButtonElement>,
  ) => {
    switch (event.key) {
      case "ArrowRight":
        event.preventDefault();
        move(index, 1);
        break;
      case "ArrowDown":
        event.preventDefault();
        move(index, 1);
        break;
      case "ArrowLeft":
        event.preventDefault();
        move(index, -1);
        break;
      case "ArrowUp":
        event.preventDefault();
        move(index, -1);
        break;
    }
  };

  return (
    <div
      className={`card-grid${className ? ` ${className}` : ""}`}
      role="group"
      aria-label={ariaLabel}
    >
      {options.map((option, index) => (
        <button
          key={option.id}
          type="button"
          ref={(node) => {
            refs.current[index] = node;
          }}
          className="card"
          data-selected={selectedId === option.id ? "true" : "false"}
          aria-pressed={selectedId === option.id}
          onClick={() => onSelect(option.id)}
          onKeyDown={(event) => handleKeyDown(index, event)}
        >
          <h3>
            {option.title}
            {option.recommended && (
              <span className="badge badge--recommended">Recommended</span>
            )}
            {option.badge && !option.recommended && (
              <span className="badge">{option.badge}</span>
            )}
          </h3>
          {option.render?.()}
          {option.description && <p>{option.description}</p>}
        </button>
      ))}
    </div>
  );
}
