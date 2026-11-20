import type { ComponentProps } from "react";

type SettingsFieldProps = ComponentProps<"div"> & {
    stacked?: boolean;
    last?: boolean;
};

export function SettingsField({ stacked, last, className, ...props }: SettingsFieldProps) {
    const classes = ["setting-item", stacked && "column", last && "no-border", className]
        .filter(Boolean).join(" ");
    return <div {...props} className={classes} />;
}
