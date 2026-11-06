import {
  createContext,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";
import { Monitor, Moon, Sun } from "lucide-react";
import { Theme } from "@radix-ui/themes";
import { SegmentedControl } from "./ui";

type Appearance = "system" | "light" | "dark";
const storageKey = "xxswitch.appearance";
const AppearanceContext = createContext<{
  appearance: Appearance;
  setAppearance: (value: Appearance) => void;
} | null>(null);

export function AppearanceProvider({ children }: { children: ReactNode }) {
  const [appearance, setAppearance] = useState<Appearance>(() => {
    try {
      const stored = localStorage.getItem(storageKey);
      return stored === "light" || stored === "dark" ? stored : "system";
    } catch {
      return "system";
    }
  });
  const [systemDark, setSystemDark] = useState(
    () => window.matchMedia("(prefers-color-scheme: dark)").matches,
  );
  const resolvedAppearance =
    appearance === "system" ? (systemDark ? "dark" : "light") : appearance;

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const update = () => setSystemDark(media.matches);
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);

  useEffect(() => {
    document.documentElement.dataset.appearance = resolvedAppearance;
  }, [resolvedAppearance]);

  useEffect(() => {
    // The main window and compact tray window share the same local origin.
    // Resync on focus as well for WebViews that defer storage events while hidden.
    const sync = () => {
      try {
        const stored = localStorage.getItem(storageKey);
        setAppearance(
          stored === "light" || stored === "dark" ? stored : "system",
        );
      } catch {
        /* Keep the current appearance when storage is unavailable. */
      }
    };
    const onStorage = (event: StorageEvent) => {
      if (event.key === storageKey || event.key === null) sync();
    };
    window.addEventListener("storage", onStorage);
    window.addEventListener("focus", sync);
    return () => {
      window.removeEventListener("storage", onStorage);
      window.removeEventListener("focus", sync);
    };
  }, []);

  useEffect(() => {
    try {
      localStorage.setItem(storageKey, appearance);
    } catch {
      /* Appearance still works without storage. */
    }
  }, [appearance]);

  return (
    <AppearanceContext.Provider value={{ appearance, setAppearance }}>
      <Theme
        className="app-theme"
        appearance={resolvedAppearance}
        accentColor="jade"
        grayColor="slate"
        radius="medium"
      >
        {children}
      </Theme>
    </AppearanceContext.Provider>
  );
}

export function AppearanceControl() {
  const context = useContext(AppearanceContext);
  if (!context)
    throw new Error("AppearanceControl requires AppearanceProvider");
  const { appearance, setAppearance } = context;
  return (
    <SegmentedControl
      className="appearance-control"
      label="界面外观"
      value={appearance}
      onChange={setAppearance}
      options={[
        {
          value: "light",
          tooltip: "浅色",
          label: (
            <span>
              <Sun size={15} aria-hidden="true" />
              <span className="sr-only">浅色</span>
            </span>
          ),
        },
        {
          value: "dark",
          tooltip: "深色",
          label: (
            <span>
              <Moon size={15} aria-hidden="true" />
              <span className="sr-only">深色</span>
            </span>
          ),
        },
        {
          value: "system",
          tooltip: "跟随系统",
          label: (
            <span>
              <Monitor size={15} aria-hidden="true" />
              <span className="sr-only">跟随系统</span>
            </span>
          ),
        },
      ]}
    />
  );
}
