import React, { useEffect, useRef } from "react";

type WheelHotZoneProps = {
  onWheel: (e: WheelEvent) => void;
  children: React.ReactNode;
};

export const WheelHotZone = ({ onWheel, children }: WheelHotZoneProps) => {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.addEventListener("wheel", onWheel, { passive: true });
    return () => el.removeEventListener("wheel", onWheel);
  }, [onWheel]);
  return (
    <div
      ref={ref}
      style={{
        flex: 1,
        overflow: "hidden",
        display: "flex",
        flexDirection: "column",
        minHeight: 0
      }}
    >
      {children}
    </div>
  );
};