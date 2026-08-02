import type { ReactNode } from "react";

import { Button } from "../../components/Button";

export interface WizardShellProps {
  step: number;
  total: number;
  canContinue: boolean;
  canGoBack: boolean;
  continueLabel?: string;
  hint?: string;
  onBack: () => void;
  onContinue: () => void;
  onGoToStep?: (step: number) => void;
  children: ReactNode;
}

/**
 * Chrome around each wizard screen: progress dots, the screen content, and the
 * Back / Continue footer. Every control is a real button, so the wizard is
 * fully keyboard-navigable (spec §19).
 */
export function WizardShell({
  step,
  total,
  canContinue,
  canGoBack,
  continueLabel = "Continue",
  hint,
  onBack,
  onContinue,
  onGoToStep,
  children,
}: WizardShellProps) {
  return (
    <div className="wizard">
      <nav className="wizard-progress" aria-label="Setup progress">
        {Array.from({ length: total }, (_, index) => (
          <button
            key={index}
            type="button"
            className="step-dot"
            data-state={
              index === step ? "current" : index < step ? "past" : "future"
            }
            aria-label={`Go to step ${index + 1} of ${total}`}
            aria-current={index === step ? "step" : undefined}
            disabled={index > step}
            onClick={() => onGoToStep?.(index)}
          />
        ))}
      </nav>

      {children}

      <footer className="wizard-nav">
        <Button
          variant="ghost"
          onClick={onBack}
          disabled={!canGoBack}
          aria-label="Go back"
        >
          Back
        </Button>
        {hint && <span className="nav-hint">{hint}</span>}
        <span className="nav-actions">
          <Button variant="primary" onClick={onContinue} disabled={!canContinue}>
            {continueLabel}
          </Button>
        </span>
      </footer>
    </div>
  );
}
