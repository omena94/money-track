import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { resolve, join } from "node:path";
import { test } from "node:test";
import { checkProject, inspectFile } from "../check-public.mjs";

test("official OTP sender exemption is exact and scoped to reviewed files", () => {
  const sender = "ebank@dfm.taipeifubon.com.tw";
  const path = "apps/collector/protocol/gmail-otp.ts";
  assert.deepEqual(inspectFile(path, Buffer.from(sender)), []);
  for (const [file, value] of [
    ["README.md", sender],
    [path, sender.replace("ebank", "personal")],
    [path, sender.replace(".tw", ".tw.evil.test-domain.com")],
  ]) {
    assert(
      inspectFile(file, Buffer.from(value)).some(
        (item) => item.kind === "non-example-email",
      ),
    );
  }
});

test("private paths and credentials are rejected without disclosing values", () => {
  const sample = "gh" + "p_" + "a".repeat(36);
  const result = inspectFile(".local/session.txt", Buffer.from(sample));
  assert(result.some((item) => item.kind === "private-or-generated-path"));
  assert(result.some((item) => item.kind === "provider-token"));
  assert(!JSON.stringify(result).includes(sample));
  assert(
    inspectFile("apps/web/.env.production", Buffer.from("placeholder")).length,
  );
  assert(
    inspectFile("apps/web/public/banks/demo.png", Buffer.from([0, 1])).length,
  );
  assert.deepEqual(
    inspectFile("README.md", Buffer.from("Contact you@example.com")),
    [],
  );
});

test("staged blobs remain checked after the worktree secret is removed", () => {
  const parent = resolve(import.meta.dirname, "../../.local");
  mkdirSync(parent, { recursive: true });
  const root = mkdtempSync(join(parent, "public-check-test-"));
  const git = (...args) =>
    execFileSync("git", args, { cwd: root, windowsHide: true, stdio: "pipe" });
  try {
    git("init", "--quiet");
    writeFileSync(join(root, ".gitignore"), ".local/\n");
    writeFileSync(join(root, "README.md"), "gh" + "p_" + "b".repeat(36));
    git("add", ".gitignore", "README.md");
    writeFileSync(join(root, "README.md"), "clean worktree");
    assert.equal(checkProject(root).findings.length, 0);
    assert(
      checkProject(root, { staged: true }).findings.some(
        (item) => item.kind === "provider-token",
      ),
    );
    mkdirSync(join(root, ".local"));
    writeFileSync(join(root, ".local", "secret.dpapi"), "encrypted test data");
    git("add", "--force", ".local/secret.dpapi");
    assert(
      checkProject(root, { staged: true }).findings.some(
        (item) => item.kind === "private-or-generated-path",
      ),
    );
  } finally {
    if (root.startsWith(join(parent, "public-check-test-")))
      rmSync(root, { recursive: true, force: true });
  }
});

test("template values are distinct from credentials and escaped personal paths", () => {
  assert(
    inspectFile(
      "README.md",
      Buffer.from(
        ["postgresql://", "user:", "private-value", "@localhost/db"].join(""),
      ),
    ).some((item) => item.kind === "credential-url"),
  );
  assert.deepEqual(
    inspectFile(
      "README.md",
      Buffer.from("postgresql://user:${generatedValue}@localhost/db"),
    ),
    [],
  );
  assert(
    inspectFile(
      "README.md",
      Buffer.from("C:" + "\\\\" + "Users" + "\\\\" + "PERSON"),
    ).some((item) => item.kind === "personal-path"),
  );
  assert(
    inspectFile(
      "apps/collector/fixtures/richart/clipped-five.png",
      Buffer.from("replaced image"),
    ).some((item) => item.kind === "unreviewed-image-change"),
  );
});

test("uninitialized projects respect ignore rules without creating a root repository", () => {
  const parent = resolve(import.meta.dirname, "../../.local");
  mkdirSync(parent, { recursive: true });
  const root = mkdtempSync(join(parent, "public-check-test-"));
  try {
    writeFileSync(join(root, ".gitignore"), ".local/\n");
    writeFileSync(join(root, "README.md"), "Public project");
    assert.deepEqual(checkProject(root), { count: 2, findings: [] });
    assert.throws(() => checkProject(root, { staged: true }));
  } finally {
    if (root.startsWith(join(parent, "public-check-test-")))
      rmSync(root, { recursive: true, force: true });
  }
});
