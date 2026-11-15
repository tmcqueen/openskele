// Since v7 dockview-core ships its stylesheet only inside this self-injecting
// bundle; the package's default ESM entry is unstyled.
declare module "dockview-core/dist/dockview-core.js" {
  export * from "dockview-core";
}
