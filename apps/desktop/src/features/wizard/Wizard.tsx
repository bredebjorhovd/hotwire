import { useCallback, useMemo, useState } from "react";

import { loadFixtureProfile } from "../catalog/fixtures";
import { WizardShell } from "./WizardShell";
import { WelcomeScreen } from "./screens/WelcomeScreen";
import { ControlSurfaceScreen } from "./screens/ControlSurfaceScreen";
import { PermissionScreen } from "./screens/PermissionScreen";
import { CaptureScreen } from "./screens/CaptureScreen";
import { ProfileScreen } from "./screens/ProfileScreen";
import { ConnectScreen } from "./screens/ConnectScreen";
import { TestScreen } from "./screens/TestScreen";
import { DoneScreen } from "./screens/DoneScreen";

export interface WizardChoices {
  controlSurface: string | null;
  permissionGranted: boolean;
  captureComplete: boolean;
  profileId: string | null;
}

export const WIZARD_STEP_COUNT = 8;

const continueLabels = [
  "Set up Hotwire",
  "Continue",
  "Continue",
  "Continue",
  "Continue",
  "Continue",
  "Continue",
  "Finish",
];

const hints: Array<string | undefined> = [
  "Eight quick steps to a useful numpad.",
  "Pick the surface you use most.",
  undefined,
  "Press the pulsing key, skip it, or use the standard layout.",
  "You can change this later in Profiles.",
  undefined,
  "Press a key to trace its route.",
  "You're all set.",
];

export interface WizardProps {
  onFinish: (choices: WizardChoices) => void;
}

/**
 * The eight-screen first-run wizard (spec §4.1). Navigation is fully keyboard
 * accessible; each step gates Continue until its requirement is met.
 */
export function Wizard({ onFinish }: WizardProps) {
  const [step, setStep] = useState(0);
  const [choices, setChoices] = useState<WizardChoices>({
    controlSurface: null,
    permissionGranted: false,
    captureComplete: false,
    profileId: null,
  });

  const canContinue = useMemo(() => {
    switch (step) {
      case 0:
        return true;
      case 1:
        return choices.controlSurface !== null;
      case 2:
        return choices.permissionGranted;
      case 3:
        return choices.captureComplete;
      case 4:
        return choices.profileId !== null;
      case 5:
      case 6:
      case 7:
        return true;
      default:
        return false;
    }
  }, [step, choices]);

  const goTo = (next: number) =>
    setStep(Math.max(0, Math.min(WIZARD_STEP_COUNT - 1, next)));

  const handleContinue = () => {
    if (step < WIZARD_STEP_COUNT - 1) {
      goTo(step + 1);
    } else {
      onFinish(choices);
    }
  };

  const activeProfile = loadFixtureProfile(
    choices.profileId ?? "ai-numpad",
  );

  const handleCaptureComplete = useCallback((complete: boolean) => {
    setChoices((c) => ({ ...c, captureComplete: complete }));
  }, []);

  return (
    <WizardShell
      step={step}
      total={WIZARD_STEP_COUNT}
      canContinue={canContinue}
      canGoBack={step > 0}
      continueLabel={continueLabels[step]}
      hint={hints[step]}
      onBack={() => goTo(step - 1)}
      onContinue={handleContinue}
      onGoToStep={goTo}
    >
      {step === 0 && <WelcomeScreen />}
      {step === 1 && (
        <ControlSurfaceScreen
          selected={choices.controlSurface}
          onSelect={(id) => setChoices((c) => ({ ...c, controlSurface: id }))}
        />
      )}
      {step === 2 && (
        <PermissionScreen
          granted={choices.permissionGranted}
          onGrant={() => setChoices((c) => ({ ...c, permissionGranted: true }))}
        />
      )}
      {step === 3 && (
        <CaptureScreen onComplete={handleCaptureComplete} />
      )}
      {step === 4 && (
        <ProfileScreen
          selected={choices.profileId}
          onSelect={(id) => setChoices((c) => ({ ...c, profileId: id }))}
        />
      )}
      {step === 5 && <ConnectScreen />}
      {step === 6 && <TestScreen profile={activeProfile} />}
      {step === 7 && <DoneScreen profile={activeProfile} />}
    </WizardShell>
  );
}
