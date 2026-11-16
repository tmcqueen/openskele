// Gives bun test a DOM (happy-dom). Loaded once by bunfig.toml before the test files.
import { GlobalRegistrator } from "@happy-dom/global-registrator";

GlobalRegistrator.register();
