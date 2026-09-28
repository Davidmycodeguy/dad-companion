import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { HoverValueCard } from "@/components/hover-card";
import type { HoverCard } from "@/lib/api";

interface CardMessage {
  seq: number;
  card: HoverCard | null;
}

/**
 * The page of the transparent card window: renders the card Rust sends and reports its size, so
 * Rust can place the window beside the game's tooltip and show it.
 */
export function CardWindow() {
  const [message, setMessage] = useState<CardMessage | null>(null);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const stop = listen<CardMessage>("hover-card", (event) => setMessage(event.payload));
    return () => {
      stop.then((unlisten) => unlisten()).catch(() => {});
    };
  }, []);

  useLayoutEffect(() => {
    if (!message?.card || !box.current) return;
    // Physical pixels: the page knows both its zoom and the display scaling.
    const { width, height } = box.current.getBoundingClientRect();
    const ratio = window.devicePixelRatio;
    invoke("card_rendered", { seq: message.seq, width: width * ratio, height: height * ratio }).catch(() => {});
  }, [message]);

  if (!message?.card) return null;
  return (
    <div ref={box} className="inline-block p-2">
      <HoverValueCard card={message.card} className="shadow-[0_4px_14px_rgb(0_0_0/0.6)]" />
    </div>
  );
}
