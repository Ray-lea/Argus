// Read-only checks through SignPath's documented API. Never log tokens,
// policy participants or response bodies that may contain account details.
import { X509Certificate } from "node:crypto";
import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const token = process.env.SIGNPATH_API_TOKEN;
const organization = process.env.SIGNPATH_ORGANIZATION_ID;
const project = process.env.SIGNPATH_PROJECT_SLUG;
const policy = process.env.SIGNPATH_POLICY_SLUG;
if (!token || !/^[a-f0-9-]{36}$/i.test(organization ?? "") || !/^[\w.-]+$/.test(project ?? "") || policy !== "test-signing") {
  throw new Error("The test signing credentials and identifiers must be configured.");
}
const base = `https://app.signpath.io/Api/v1/${organization}/Projects/${project}`;
async function get(path, accept = "application/json") {
  const response = await fetch(`${base}/${path}`, {
    headers: { Authorization: `Bearer ${token}`, Accept: accept },
    redirect: "error",
    signal: AbortSignal.timeout(30_000),
  });
  if (!response.ok) throw new Error(`SignPath preflight ${path}: HTTP ${response.status}`);
  return response;
}
const settings = await (await get(`SigningPolicies/${policy}`)).json();
if (settings.status !== "Valid" || settings.purpose !== "TestSigning") {
  throw new Error("A valid test-signing policy is required.");
}
console.log(`Test policy valid; required approvals: ${settings.numberOfRequiredApprovals}`);
for (const slug of ["windows-helpers", "windows-application", "windows-installer"]) {
  await get(`ArtifactConfigurations/${slug}/Xml`, "application/xml");
  console.log(`Artifact configuration available: ${slug}`);
}
const certificate = new X509Certificate(Buffer.from(await (
  await get(`SigningPolicies/${policy}/Certificate/X509Certificate`, "application/pkix-cert")
).arrayBuffer()));
if (certificate.subject !== certificate.issuer || !certificate.verify(certificate.publicKey)
    || !certificate.keyUsage?.includes("1.3.6.1.5.5.7.3.3")) {
  throw new Error("The test policy must provide a self-signed code-signing certificate.");
}
const stage = resolve(import.meta.dirname, "..", "target", "signpath");
mkdirSync(stage, { recursive: true });
writeFileSync(join(stage, "test-certificate.cer"), certificate.raw);
console.log(`Test certificate pinned: ${certificate.fingerprint256}`);
