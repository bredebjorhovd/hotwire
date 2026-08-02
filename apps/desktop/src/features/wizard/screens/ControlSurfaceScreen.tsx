import { CardGrid, type CardOption } from "../../../components/CardGrid";
import { MiniNumpad } from "../../../components/MiniNumpad";
import { loadFixtureProfile } from "../../catalog/fixtures";

const options: CardOption[] = [
  {
    id: "numpad",
    title: "Numpad",
    description: "Full-size ISO layout with a dedicated numpad.",
    recommended: true,
    render: () => <MiniNumpad profile={loadFixtureProfile("ai-numpad")} />,
  },
  {
    id: "function_row",
    title: "Function row",
    description: "The F1–F12 keys on a laptop or compact board.",
  },
  {
    id: "macropad",
    title: "External macropad",
    description: "A standalone pad on your desk.",
  },
  {
    id: "manual",
    title: "Choose keys manually",
    description: "Pick any physical keys one at a time.",
  },
];

export interface ControlSurfaceScreenProps {
  selected: string | null;
  onSelect: (id: string) => void;
}

/** Screen 2 — Select a control surface (spec §4.1). */
export function ControlSurfaceScreen({
  selected,
  onSelect,
}: ControlSurfaceScreenProps) {
  return (
    <section className="screen" aria-labelledby="surface-title">
      <p className="eyebrow">Setup · 2 of 8</p>
      <h1 className="screen-title" id="surface-title">
        Which keys will drive Hotwire?
      </h1>
      <p className="screen-lede">
        The board adapts to the surface you already own. You can change this
        later.
      </p>
      <CardGrid
        options={options}
        selectedId={selected}
        onSelect={onSelect}
        ariaLabel="Control surfaces"
      />
    </section>
  );
}
