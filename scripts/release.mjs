// release.mjs — bump the version everywhere it appears, commit, and tag.
//
// The version lives in four places that must agree: package.json,
// package-lock.json, src-tauri/tauri.conf.json, and src-tauri/Cargo.toml
// (which also rewrites Cargo.lock). Updating them by hand drifts, and a
// mismatch between the tag and tauri.conf.json produces a release the updater
// silently ignores.
//
//   npm run release -- 0.2.0
import { readFileSync, writeFileSync } from "node:fs";
import { execSync } from "node:child_process";

const version = process.argv[2];

if (!version || !/^\d+\.\d+\.\d+$/.test(version)) {
  console.error("usage: npm run release -- <major.minor.patch>   e.g. 0.2.0");
  process.exit(1);
}

function run(cmd) {
  return execSync(cmd, { encoding: "utf8" }).trim();
}

// Refuse to tag a dirty tree: the tag should point at exactly what was built.
if (run("git status --porcelain")) {
  console.error("working tree is dirty — commit or stash first");
  process.exit(1);
}

const branch = run("git rev-parse --abbrev-ref HEAD");
if (branch !== "main") {
  console.error(`releases are cut from main; you are on '${branch}'`);
  process.exit(1);
}

if (run(`git tag -l v${version}`)) {
  console.error(`tag v${version} already exists`);
  process.exit(1);
}

/* ---------- bump ---------- */

function editJson(path, fn) {
  const obj = JSON.parse(readFileSync(path, "utf8"));
  fn(obj);
  writeFileSync(path, JSON.stringify(obj, null, 2) + "\n");
  console.log(`  ${path}`);
}

console.log(`bumping to ${version}:`);
editJson("package.json", (p) => (p.version = version));
editJson("package-lock.json", (p) => {
  p.version = version;
  if (p.packages?.[""]) p.packages[""].version = version;
});
editJson("src-tauri/tauri.conf.json", (c) => (c.version = version));

// Only the [package] version, not any dependency that happens to match.
const cargoPath = "src-tauri/Cargo.toml";
const cargo = readFileSync(cargoPath, "utf8");
const bumped = cargo.replace(/^version = "[^"]+"/m, `version = "${version}"`);
if (bumped === cargo) {
  console.error("could not find the version line in Cargo.toml");
  process.exit(1);
}
writeFileSync(cargoPath, bumped);
console.log(`  ${cargoPath}`);

// Keep Cargo.lock in step so the build does not rewrite it mid-release.
execSync("cargo update --workspace --offline", { cwd: "src-tauri", stdio: "ignore" });
console.log("  src-tauri/Cargo.lock");

/* ---------- commit and tag ---------- */

execSync("git add -A", { stdio: "inherit" });
execSync(`git commit -m "Release v${version}"`, { stdio: "inherit" });
execSync(`git tag v${version}`, { stdio: "inherit" });

console.log(`
Tagged v${version}. Push it to trigger the release build:

  git push && git push origin v${version}

CI will build Windows and macOS, sign them, and publish the release.
`);
