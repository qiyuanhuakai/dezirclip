export const shouldRequestSystemFonts = (collapsed: boolean, hasLoaded: boolean) =>
  !collapsed && !hasLoaded;
