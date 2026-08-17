import { useMemo, useState } from "react";
import { Card, Divider, Field, NumberInput, SectionTitle, Select, Switch } from "../components/ui";
import type { FillMode, Order, Theme } from "../lib/types";
import { useApp } from "../state/AppContext";

type Unit = "seconds" | "minutes" | "hours";

const UNIT_SECONDS: Record<Unit, number> = {
  seconds: 1,
  minutes: 60,
  hours: 3600,
};

const MIN_INTERVAL_SECONDS = 10;

/** Shows the interval in the largest unit that divides it exactly. */
function splitInterval(totalSeconds: number): { value: number; unit: Unit } {
  if (totalSeconds % UNIT_SECONDS.hours === 0) return { value: totalSeconds / UNIT_SECONDS.hours, unit: "hours" };
  if (totalSeconds % UNIT_SECONDS.minutes === 0) return { value: totalSeconds / UNIT_SECONDS.minutes, unit: "minutes" };
  return { value: totalSeconds, unit: "seconds" };
}

export function Settings() {
  const { config, environment, updateConfig } = useApp();
  const derived = useMemo(() => (config ? splitInterval(config.intervalSeconds) : null), [config]);
  const [unit, setUnit] = useState<Unit | null>(null);

  if (!config || !derived) return null;

  const activeUnit = unit ?? derived.unit;
  const intervalValue =
    activeUnit === derived.unit
      ? derived.value
      : Math.max(1, Math.round(config.intervalSeconds / UNIT_SECONDS[activeUnit]));

  const setInterval = (value: number, nextUnit: Unit) => {
    setUnit(nextUnit);
    void updateConfig({
      intervalSeconds: Math.max(MIN_INTERVAL_SECONDS, value * UNIT_SECONDS[nextUnit]),
    });
  };

  return (
    <div className="mx-auto flex max-w-2xl flex-col gap-6 px-8 py-8">
      <section className="flex flex-col gap-3">
        <SectionTitle>Rotation</SectionTitle>
        <Card>
          <Field
            label="Change every"
            hint="How long each wallpaper stays on screen."
            control={
              <div className="flex items-center gap-2">
                <NumberInput
                  label="Interval"
                  value={intervalValue}
                  min={activeUnit === "seconds" ? MIN_INTERVAL_SECONDS : 1}
                  onChange={value => setInterval(value, activeUnit)}
                />
                <Select
                  aria-label="Interval unit"
                  value={activeUnit}
                  onChange={event => setInterval(intervalValue, event.target.value as Unit)}
                >
                  <option value="seconds">seconds</option>
                  <option value="minutes">minutes</option>
                  <option value="hours">hours</option>
                </Select>
              </div>
            }
          />
          <Divider />
          <Field
            label="Order"
            hint="Shuffle plays every image once before repeating."
            control={
              <Select
                aria-label="Order"
                value={config.order}
                onChange={event => void updateConfig({ order: event.target.value as Order })}
              >
                <option value="shuffle">Shuffle</option>
                <option value="sequential">Sequential</option>
                <option value="random">Random</option>
              </Select>
            }
          />
          <Divider />
          <Field
            label="Change on startup"
            hint="Pick a new wallpaper as soon as the app starts."
            control={
              <Switch
                label="Change on startup"
                checked={config.changeOnStartup}
                onChange={changeOnStartup => void updateConfig({ changeOnStartup })}
              />
            }
          />
        </Card>
      </section>

      <section className="flex flex-col gap-3">
        <SectionTitle>Appearance on screen</SectionTitle>
        <Card>
          <Field
            label="Fill mode"
            hint="How an image is scaled to your screen."
            control={
              <Select
                aria-label="Fill mode"
                value={config.fillMode}
                onChange={event => void updateConfig({ fillMode: event.target.value as FillMode })}
              >
                <option value="fill">Fill</option>
                <option value="fit">Fit</option>
                <option value="stretch">Stretch</option>
                <option value="center">Center</option>
                <option value="tile">Tile</option>
              </Select>
            }
          />
          <Divider />
          <Field
            label="Different image per monitor"
            hint={
              environment?.supportsPerMonitor
                ? `${environment.monitorCount} monitor${environment.monitorCount === 1 ? "" : "s"} detected.`
                : `Not supported on ${environment?.backend ?? "this desktop"}.`
            }
            control={
              <Switch
                label="Different image per monitor"
                checked={config.perMonitor}
                disabled={!environment?.supportsPerMonitor}
                onChange={perMonitor => void updateConfig({ perMonitor })}
              />
            }
          />
        </Card>
      </section>

      <section className="flex flex-col gap-3">
        <SectionTitle>Application</SectionTitle>
        <Card>
          <Field
            label="Start at login"
            hint="Keeps rotating your wallpaper after every reboot."
            control={
              <Switch
                label="Start at login"
                checked={config.autostart}
                onChange={autostart => void updateConfig({ autostart })}
              />
            }
          />
          <Divider />
          <Field
            label="Start in the tray"
            hint="When started at login, stay hidden instead of opening this window."
            control={
              <Switch
                label="Start in the tray"
                checked={config.startMinimized}
                disabled={!config.autostart}
                onChange={startMinimized => void updateConfig({ startMinimized })}
              />
            }
          />
          <Divider />
          <Field
            label="Theme"
            control={
              <Select
                aria-label="Theme"
                value={config.theme}
                onChange={event => void updateConfig({ theme: event.target.value as Theme })}
              >
                <option value="system">Match system</option>
                <option value="light">Light</option>
                <option value="dark">Dark</option>
              </Select>
            }
          />
        </Card>
        <p className="px-1 text-xs text-ink-muted">
          Closing this window leaves Wallflow running in the tray. Use Quit in the tray menu to stop it completely.
        </p>
      </section>
    </div>
  );
}
