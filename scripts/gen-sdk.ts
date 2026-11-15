// Generates each SDK's protocol types from protocol/openrpc.json. The
// emitters handle the subset of JSON Schema that the protocol document uses.

import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { emitCsharp } from "./gen/csharp";
import { root } from "./gen/model";
import { emitTypescript } from "./gen/typescript";

const targets: [string, () => string][] = [
  ["sdk/typescript/src/generated/protocol.ts", emitTypescript],
  ["sdk/dotnet/Skele.Sdk/Generated/Protocol.g.cs", emitCsharp],
];

for (const [path, emit] of targets) {
  writeFileSync(join(root, path), emit());
  console.log(`wrote ${path}`);
}
