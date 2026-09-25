import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
} from "node:fs";
import { dirname, extname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const rootFiles = new Set([
  ".gitignore",
  ".gitattributes",
  "AGENTS.md",
  "README.md",
  "LICENSE",
  "THIRD_PARTY_NOTICES.md",
  "CONTRIBUTING.md",
  "SECURITY.md",
  "package.json",
  "package-lock.json",
  "Cargo.toml",
  "Cargo.lock",
]);
const textExtensions = new Set([
  ".md",
  ".json",
  ".toml",
  ".lock",
  ".ts",
  ".tsx",
  ".mjs",
  ".css",
  ".svg",
  ".rs",
  ".sql",
  ".py",
  ".ps1",
  ".txt",
]);
const publicImages = new Map(
  [
    [
      "clipped-five.png",
      "7cb0e57b1673f827283d8b5cb39b0014bc183f5f1fc71005f7b713db2cb2b2c0",
    ],
    [
      "clipped-six.png",
      "0f236ed0ecfe0656c45c3b925cbfbadf21c89a3517a3e73b30087cd7acc21342",
    ],
    [
      "conflicting-six.png",
      "caaae1a1127a97324b7e32ae2ad02dfd44d6dbff9ff268f4e77ff5c4566a3dba",
    ],
    [
      "low-confidence-six.png",
      "c469b42fb870642a81b04a1d8775781fc374369bcabb4c5dbf1066d104b5c433",
    ],
    [
      "uncertain-five.png",
      "2f400264227580f341d94b30bf8f2089a4bc9150fe5281d067fe09fd8fe9b21a",
    ],
  ].map(([name, hash]) => [`apps/collector/fixtures/richart/${name}`, hash]),
);
const privatePath =
  /(?:^|\/)(?:\.local|\.git|\.venv|node_modules|target|dist|\.next|__pycache__|research|private|backups|screenshots|coverage|playwright-report|test-results)(?:\/|$)|(?:^|\/)\.env(?!\.example$)|\.(?:dpapi|mtbackup|pem|key|p12|pfx|har|dump|db|sqlite\w*|log|tsbuildinfo|bak|backup|zip|7z|apk|pyc)$/i;
const privateTool =
  /(?:research|commission|protocol_probe|credentials_local|read_private_state|oauth_local_setup|Configure-Tunnel|vendor-connectors|diagnose_hsbc|verify_hsbc|link_confirmed_accounts|remove_disconnected_banks)/i;
const rules = [
  [
    "private-key",
    /-----BEGIN (?:RSA |EC |OPENSSH |DSA |ENCRYPTED )?PRIVATE KEY-----/,
  ],
  [
    "provider-token",
    /\b(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,}|GOCSPX-[A-Za-z0-9_-]{20,}|AKIA[A-Z0-9]{16}|xox[baprs]-[A-Za-z0-9-]{20,})\b/,
  ],
  ["google-client-id", /\b\d{8,}-[a-z0-9]+\.apps\.googleusercontent\.com\b/i],
  ["credential-url", /[a-z][a-z0-9+.-]*:\/\/[^\s/:"'<>]+:[^\s/@"'<>${}]+@/i],
  [
    "personal-path",
    /[A-Z]:[\\/]+(?:Users[\\/]+(?!Public\b|Default\b)[^\s"'<>]+|Work[\\/]+)/i,
  ],
  [
    "jwt-literal",
    /\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b/,
  ],
  [
    "private-dependency",
    /(?:from\s+oauth_local_setup\s+import|\.local[\\/]research[\\/]|\.\.\/all-set-tw\/)/,
  ],
  [
    "literal-secret",
    /\b(?:password|passwd|client_secret|api_key|api_token|access_token|refresh_token)\b["']?\s*[:=]\s*["'][A-Za-z0-9_+\/-]{20,}["']/i,
  ],
];

// Exact, reviewed synthetic passwords; do not exempt whole test files.
const syntheticSecrets = new Map([
  ["apps/api/tests/financial_sources.rs", "fixture-private-password"],
]);

// Reviewed official sender address required by the Fubon OTP allowlist.
// This does not exempt personal mailbox addresses or other files.
const publicSenderFiles = new Set([
  "scripts/check-public.mjs",
  "scripts/tests/check-public.test.mjs",
]);

// Only paths, line numbers and categories leave the scanner, never matching values.
export function inspectFile(path, bytes, mode = "100644") {
  const findings = [];
  const report = (kind, line) =>
    findings.push({ path, kind, ...(line ? { line } : {}) });
  if (!["100644", "100755"].includes(mode)) report("unsupported-file-mode");
  if (privatePath.test(path) || privateTool.test(path))
    report("private-or-generated-path");
  if (!rootFiles.has(path) && !/^(?:apps|docs|scripts)\//.test(path))
    report("unreviewed-root-file");
  if (
    path.startsWith("apps/api/examples/") &&
    !["openapi.rs", "ui_fixture.rs"].includes(path.split("/").at(-1))
  )
    report("private-example");
  if (bytes.length > 5 * 1024 * 1024) report("oversized-file");
  if (publicImages.has(path)) {
    if (
      createHash("sha256").update(bytes).digest("hex") !==
      publicImages.get(path)
    )
      report("unreviewed-image-change");
    return findings;
  }
  if (
    !rootFiles.has(path) &&
    !textExtensions.has(extname(path)) &&
    path.split("/").at(-1) !== "LICENSE"
  )
    report("unreviewed-file-type");
  if (bytes.includes(0)) {
    report("unreviewed-binary");
    return findings;
  }
  const lines = bytes.toString("utf8").split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    for (const [kind, pattern] of rules) {
      const line =
        kind === "literal-secret" && syntheticSecrets.has(path)
          ? lines[i].replaceAll(syntheticSecrets.get(path), "fixture")
          : lines[i];
      if (pattern.test(line)) report(kind, i + 1);
    }
    for (const email of lines[i].matchAll(
      /\b[A-Za-z0-9._%+-]+@([A-Za-z0-9.-]+\.[A-Za-z]{2,})\b/g,
    )) {
      if (
        publicSenderFiles.has(path) &&
        email[0] === "ebank@dfm.taipeifubon.com.tw"
      )
        continue;
      // Reserved/example domains and localhost-only fixture identities are intentional.
      if (
        !/(?:^|\.)(?:example\.(?:com|org|net)|test|invalid|localhost)$/.test(
          email[1],
        )
      )
        report("non-example-email", i + 1);
    }
  }
  return findings;
}

export function checkProject(root, { staged = false } = {}) {
  root = resolve(root);
  const run = (args, options = {}) =>
    execFileSync("git", args, {
      cwd: root,
      windowsHide: true,
      stdio: ["pipe", "pipe", "pipe"],
      maxBuffer: 32 * 1024 * 1024,
      ...options,
    });
  let temporary;
  let prefix = [];
  const hasGit = existsSync(join(root, ".git"));
  if (staged && !hasGit) throw new Error("staged-check-requires-git");
  try {
    if (!hasGit) {
      const local = join(root, ".local");
      mkdirSync(local, { recursive: true });
      temporary = mkdtempSync(join(local, "publication-index-"));
      run(["init", "--bare", "--quiet", temporary]);
      prefix = [`--git-dir=${temporary}`, `--work-tree=${root}`];
    }
    const git = (args, options) => run([...prefix, ...args], options);
    const split = (buffer) =>
      buffer.toString("utf8").split("\0").filter(Boolean);
    const modes = new Map(
      split(git(["ls-files", "--stage", "-z"])).map((entry) => {
        const [metadata, ...rest] = entry.split("\t");
        const [mode, , stage] = metadata.split(" ");
        return [rest.join("\t"), stage === "0" ? mode : "unmerged"];
      }),
    );
    const paths = staged
      ? [...modes.keys()]
      : [
          ...new Set(
            split(
              git([
                "ls-files",
                "--cached",
                "--others",
                "--exclude-standard",
                "-z",
              ]),
            ),
          ),
        ];
    const findings = [];
    for (const path of paths) {
      let bytes;
      let mode = modes.get(path) ?? "100644";
      if (staged) {
        if (!["100644", "100755"].includes(mode)) {
          findings.push({ path, kind: "unsupported-file-mode" });
          continue;
        }
        bytes = git(["show", `:${path}`]);
      } else {
        const full = resolve(root, path);
        if (
          !full.startsWith(root + (process.platform === "win32" ? "\\" : "/"))
        )
          throw new Error("path-outside-project");
        if (!existsSync(full)) continue; // Deletion is checked against the index with --staged.
        if (!lstatSync(full).isFile()) {
          findings.push({ path, kind: "unsupported-file-mode" });
          continue;
        }
        bytes = readFileSync(full);
      }
      findings.push(...inspectFile(path, bytes, mode));
    }
    return { count: paths.length, findings };
  } finally {
    if (
      temporary &&
      dirname(temporary) === join(root, ".local") &&
      temporary.startsWith(join(root, ".local", "publication-index-"))
    )
      rmSync(temporary, { recursive: true, force: true });
  }
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  try {
    const unknown = process.argv.slice(2).filter((arg) => arg !== "--staged");
    if (unknown.length) throw new Error("unknown-option");
    const result = checkProject(
      resolve(dirname(fileURLToPath(import.meta.url)), ".."),
      { staged: process.argv.includes("--staged") },
    );
    for (const item of result.findings)
      console.error(
        `${item.path}${item.line ? `:${item.line}` : ""} [${item.kind}]`,
      );
    console.log(
      `Publication check: ${result.count} files, ${result.findings.length} findings. Manual review still required.`,
    );
    process.exitCode = result.findings.length ? 1 : 0;
  } catch {
    console.error(
      "Publication check did not complete. Check Git availability, repository state and options; --staged requires an initialized repository. No matched values were logged.",
    );
    process.exitCode = 1;
  }
}
