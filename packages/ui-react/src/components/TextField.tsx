import type { ComponentPropsWithRef } from "react";

export function TextField({ className = "", ...props }: ComponentPropsWithRef<"input">) {
  return <input className={["deziro-text-field", className].filter(Boolean).join(" ")} {...props} />;
}

export function NumberField(props: Omit<ComponentPropsWithRef<"input">, "type" | "inputMode" | "pattern">) {
  const { className = "", ...input } = props;
  return <TextField className={["numeric-input", className].filter(Boolean).join(" ")}
    type="text" inputMode="numeric" pattern="[0-9]*" {...input} />;
}

export function TextArea({ className = "", ...props }: ComponentPropsWithRef<"textarea">) {
  return <textarea className={["deziro-text-field", className].filter(Boolean).join(" ")} {...props} />;
}
