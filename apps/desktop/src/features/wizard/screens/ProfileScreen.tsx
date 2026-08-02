import { CardGrid, type CardOption } from "../../../components/CardGrid";
import { MiniNumpad } from "../../../components/MiniNumpad";
import { loadFixtureProfile } from "../../catalog/fixtures";

const starters: Array<{
  id: string;
  title: string;
  description: string;
  recommended?: boolean;
}> = [
  {
    id: "ai-numpad",
    title: "AI Numpad",
    description: "Voice, Herdr and agent actions on the classic layout.",
    recommended: true,
  },
  {
    id: "herdr-numpad",
    title: "Herdr",
    description: "Focus Herdr and drive agents from the board.",
  },
  {
    id: "claude-numpad",
    title: "Claude Code",
    description: "Launch Claude Code and continue sessions.",
  },
  {
    id: "codex-numpad",
    title: "Codex",
    description: "Launch Codex CLI and run the test suite.",
  },
  {
    id: "voice-apps-numpad",
    title: "Voice + Apps",
    description: "A Papegøye hold key plus app focus.",
  },
  {
    id: "blank",
    title: "Blank",
    description: "Start from an empty board.",
  },
];

const options: CardOption[] = starters.map((starter) => ({
  id: starter.id,
  title: starter.title,
  description: starter.description,
  recommended: starter.recommended,
  render: () => <MiniNumpad profile={loadFixtureProfile(starter.id)} />,
}));

export interface ProfileScreenProps {
  selected: string | null;
  onSelect: (id: string) => void;
}

/** Screen 5 — Choose a starting profile (spec §4.1). */
export function ProfileScreen({ selected, onSelect }: ProfileScreenProps) {
  return (
    <section className="screen" aria-labelledby="profile-title">
      <p className="eyebrow">Setup · 5 of 8</p>
      <h1 className="screen-title" id="profile-title">
        Choose a starting profile
      </h1>
      <p className="screen-lede">
        Each card shows its mapped numpad. You can duplicate, rename and export
        profiles later.
      </p>
      <CardGrid
        className="card-grid--profiles"
        options={options}
        selectedId={selected}
        onSelect={onSelect}
        ariaLabel="Starting profiles"
      />
    </section>
  );
}
