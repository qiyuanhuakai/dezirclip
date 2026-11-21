import type { ComponentProps } from "react";

type SettingsFieldProps = ComponentProps<"div"> & {
    stacked?: boolean;
    last?: boolean;
    detail?: boolean;
};

export function SettingsField({ stacked, last, detail, className, ...props }: SettingsFieldProps) {
    const classes = ["setting-item", stacked && "column", last && "no-border", detail && "setting-item--detail", className]
        .filter(Boolean).join(" ");
    return <div {...props} className={classes} />;
}
