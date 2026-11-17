import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";

interface Tray {
  recording: boolean;
  /** The tray menu asked to start or stop the recording. */
  onToggleRecording: () => void;
  /** The tray menu asked to quit. */
  onQuit: () => void;
}

/** Connects the window to the tray icon: its menu commands, and what it shows about the recording. */
export function useTray(tray: Tray) {
  const latest = useRef(tray);
  latest.current = tray;

  useEffect(() => {
    const stops = [
      listen("tray-record", () => latest.current.onToggleRecording()),
      listen("tray-quit", () => latest.current.onQuit()),
    ];
    return () => stops.forEach((stop) => void stop.then((unlisten) => unlisten()));
  }, []);

  useEffect(() => {
    invoke("set_tray_recording", { recording: tray.recording }).catch(() => {
      // Only the tray menu text is out of date.
    });
  }, [tray.recording]);
}

export function quitApp(): Promise<void> {
  return invoke("quit_app");
}

/** Whether the app starts hidden in the tray when Windows starts; null while unknown. */
export function useAutostart() {
  const [enabled, setEnabled] = useState<boolean | null>(null);
  useEffect(() => {
    invoke<boolean>("get_autostart").then(setEnabled, () => setEnabled(null));
  }, []);
  return {
    enabled,
    change: (next: boolean) => invoke<boolean>("set_autostart", { enabled: next }).then(setEnabled),
  };
}
