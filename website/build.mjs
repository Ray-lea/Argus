import { execFileSync } from "node:child_process";
import {
  copyFile,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  rename,
  rm,
  writeFile,
} from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout as delay } from "node:timers/promises";
import { buildRelease, filenames, origin, repo } from "./release.mjs";
import { fromTag, markdown, releasedDocs } from "./documents.mjs";
import {
  article,
  docPage,
  downloads,
  home,
  layout,
  searchPage,
} from "./pages.mjs";

const source = dirname(fileURLToPath(import.meta.url)),
  root = dirname(source),
  work = join(root, "target", "official-web");
const output = join(work, "dist");
const contact = JSON.parse(
  await readFile(join(source, "public-settings.json"), "utf8"),
).emailRoutingVerified;
await mkdir(work, { recursive: true });
const stage = await mkdtemp(join(work, "build-"));
const dir = Object.fromEntries(
  ["site", "downloads", "updates", "www", "packages"].map((k) => [
    k,
    join(stage, k),
  ]),
);
await Promise.all(Object.values(dir).map((d) => mkdir(d, { recursive: true })));
async function put(base, path, content) {
  const file = join(base, path);
  await mkdir(dirname(file), { recursive: true });
  await writeFile(file, content);
}
const urls = [];
async function page(path, html) {
  urls.push(path);
  await put(dir.site, path.slice(1) + "index.html", html);
}
const protect =
  "  X-Content-Type-Options: nosniff\n  Referrer-Policy: strict-origin-when-cross-origin\n";
const release = await buildRelease(
  root,
  join(work, "cache"),
  dir.downloads,
  dir.updates,
);
const { tag, version, files } = release;
const docs = releasedDocs(root, tag);
await put(
  dir.site,
  "assets/argus.svg",
  await readFile(join(root, "resources/logo.svg")),
);
await Promise.all(
  ["site.css", "search.js"].map((name) =>
    copyFile(join(source, "assets", name), join(dir.site, "assets", name)),
  ),
);
for (const lang of ["zh", "en"])
  for (const tone of ["light", "dark"]) {
    await put(
      dir.site,
      `assets/preview-${lang}-${tone}.png`,
      fromTag(
        root,
        tag,
        `docs/images/screenshot-${lang === "en" ? "en-" : ""}${tone}.png`,
      ),
    );
  }
const policy = await readFile(join(root, "PRIVACY.md"), "utf8");
for (const lang of ["zh", "en"]) {
  const en = lang === "en",
    prefix = en ? "/en" : "",
    readme = await readFile(
      join(root, en ? "README.en.md" : "README.md"),
      "utf8",
    );
  await page(`${prefix}/`, home(lang, release, contact));
  const stableReadme = fromTag(
    root,
    tag,
    en ? "README.en.md" : "README.md",
  ).toString("utf8");
  const section = stableReadme.match(
    en
      ? /^## [^\n]*system requirements[^\n]*\n([\s\S]*?)(?=^## )/im
      : /^## [^\n]*系统要求[^\n]*\n([\s\S]*?)(?=^## )/m,
  );
  const requirements = section
    ? markdown(
        `## ${en ? "System requirements" : "系统要求"}\n${section[1]}`,
        (url) => url.replace(/^docs\/([^#]+)\.md/, "/docs/$1/"),
      ).html
    : "";
  await page(
    `${prefix}/download/`,
    downloads(lang, release, contact, requirements),
  );
  const signing = readme.match(
    /^## [^\n]*code signing policy[^\n]*\n([\s\S]*?)(?=^## |$(?![\s\S]))/im,
  );
  if (!signing) throw Error(`Missing ${lang} signing policy`);
  const text = signing[0].replace(
    /^## [^\n]+/,
    en ? "# Code signing policy" : "# 代码签名策略",
  );
  const rendered = markdown(text, (url) =>
    url.startsWith("PRIVACY.md") ? `${prefix}/privacy/` : url,
  ).html;
  await page(
    `${prefix}/signing/`,
    article(
      lang,
      "signing",
      en ? "Code signing policy" : "代码签名策略",
      rendered,
      contact,
    ),
  );
  const split = policy.split(/^## English\s*$/m);
  if (split.length !== 2) throw Error("Missing bilingual privacy policy");
  const privateText = en ? split[1] : split[0].split(/^## 简体中文\s*$/m)[1];
  const privacy = markdown(
    `${en ? "# Privacy policy" : "# 隐私政策"}\n\n${privateText}`,
    (url) => (url.startsWith("docs/") ? `${repo}/blob/main/${url}` : url),
  ).html;
  await page(
    `${prefix}/privacy/`,
    article(
      lang,
      "privacy",
      en ? "Privacy policy" : "隐私政策",
      privacy,
      contact,
    ),
  );
  const report = `${repo}/security/advisories/new`;
  const sec = en
    ? `# Report a vulnerability\n\nSend the affected version, impact and reproduction steps through [GitHub private vulnerability reporting](${report}).`
    : `# 安全报告\n\n请通过 [GitHub 私密漏洞报告](${report}) 提交受影响的版本、影响范围与复现步骤。`;
  const email = contact
    ? "\n\n[security@argustask.com](mailto:security@argustask.com)"
    : "";
  await page(
    `${prefix}/security/`,
    article(
      lang,
      "security",
      en ? "Security" : "安全报告",
      markdown(sec + email).html,
      contact,
    ),
  );
}
for (const item of docs)
  await page(item.path, docPage(item, docs, release, contact));
await page("/search/", searchPage(contact));
await put(
  dir.site,
  "assets/search-index.json",
  JSON.stringify(docs.map(({ title, path, text }) => ({ title, path, text }))),
);
await put(
  dir.site,
  "sitemap.xml",
  `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">${urls
    .filter((u) => u !== "/search/")
    .map((u) => `<url><loc>${origin + u}</loc></url>`)
    .join("")}</urlset>\n`,
);
await put(
  dir.site,
  "robots.txt",
  `User-agent: *\nAllow: /\nSitemap: ${origin}/sitemap.xml\n`,
);
const expires = new Date(Date.now() + 350 * 86400000)
  .toISOString()
  .replace(/\.\d{3}Z$/, "Z");
await put(
  dir.site,
  ".well-known/security.txt",
  `Contact: ${repo}/security/advisories/new\n${contact ? "Contact: mailto:security@argustask.com\n" : ""}Expires: ${expires}\nCanonical: ${origin}/.well-known/security.txt\nPolicy: ${origin}/security/\nPreferred-Languages: zh, en\n`,
);
await put(
  dir.site,
  "404.html",
  layout({
    title: "页面未找到",
    path: "/404.html",
    contact,
    body: '<section class="wrap paper"><div class="eyebrow">404</div><h1>页面未找到</h1><p><a href="/">返回首页</a> · <a href="/docs/">浏览文档</a></p></section>',
  }),
);
await put(
  dir.site,
  "_headers",
  `/*\n${protect}  Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'\n  Permissions-Policy: camera=(), microphone=(), geolocation=()\n  Cache-Control: public, max-age=300\n`,
);
await put(
  dir.updates,
  "_headers",
  `/*\n${protect}  Cache-Control: public, max-age=120, must-revalidate\n`,
);
await put(
  dir.downloads,
  "_headers",
  `/*\n${protect}  Cache-Control: public, max-age=31536000, immutable\n  Content-Disposition: attachment\n`,
);
const redirects = ["/ https://argustask.com/download/ 302"];
const tags = execFileSync("git", ["tag", "--list", "v*"], {
  cwd: root,
  encoding: "utf8",
})
  .trim()
  .split(/\r?\n/);
for (const releaseTag of new Set([...tags, tag, "v" + release.beta])) {
  let names;
  try {
    names = filenames(releaseTag.slice(1));
  } catch {
    continue;
  }
  const actual = await readdir(join(dir.downloads, releaseTag)).catch(
    (error) => {
      if (error.code === "ENOENT") return [];
      throw error;
    },
  );
  for (const name of [...Object.values(names), "SHA256SUMS.txt"])
    if (!actual.includes(name)) {
      redirects.push(
        `/${releaseTag}/${name} ${repo}/releases/download/${releaseTag}/${name} 302`,
      );
    }
}
if (redirects.length > 2000)
  throw Error(
    "Static redirect limit reached; preserve versioned paths before deploying",
  );
await put(dir.downloads, "_redirects", redirects.join("\n") + "\n");
await put(
  dir.www,
  "index.html",
  '<!doctype html><title>Argus</title><a href="https://argustask.com/">Argus</a>',
);
await put(dir.www, "_redirects", "/* https://argustask.com/:splat 301\n");

// Generate submission drafts outside the hosted assets. Package managers use immutable upstream URLs.
const id = "Ray-lea.Argus",
  schema = "1.10.0",
  common = `PackageIdentifier: ${id}\nPackageVersion: ${JSON.stringify(version)}\n`;
const yaml = (type) =>
  `# yaml-language-server: $schema=https://aka.ms/winget-manifest.${type}.${schema}.schema.json\n\n` +
  common;
const winget = `winget/manifests/r/Ray-lea/Argus/${version}/`;
await put(
  dir.packages,
  winget + id + ".yaml",
  yaml("version") +
    `DefaultLocale: en-US\nManifestType: version\nManifestVersion: ${schema}\n`,
);
await put(
  dir.packages,
  winget + id + ".installer.yaml",
  yaml("installer") +
    `InstallerType: nullsoft\nScope: machine\nInstallerSwitches:\n  Silent: /S\n  SilentWithProgress: /S\nInstallers:\n  - Architecture: x64\n    InstallerUrl: ${files.installer.upstream}\n    InstallerSha256: ${files.installer.sha256.toUpperCase()}\nManifestType: installer\nManifestVersion: ${schema}\n`,
);
await put(
  dir.packages,
  winget + id + ".locale.en-US.yaml",
  yaml("defaultLocale") +
    `PackageLocale: en-US\nPublisher: Argus contributors\nPackageName: Argus\nPackageUrl: ${origin}/en/\nLicense: AGPL-3.0-or-later\nLicenseUrl: ${repo}/blob/${tag}/LICENSE\nShortDescription: A Windows task manager for processes, hardware and resource history.\nReleaseNotesUrl: ${repo}/releases/tag/${tag}\nManifestType: defaultLocale\nManifestVersion: ${schema}\n`,
);
await put(
  dir.packages,
  "scoop/argus.json",
  JSON.stringify(
    {
      version,
      description:
        "Windows task manager for processes, hardware and resource history",
      homepage: origin,
      license: "AGPL-3.0-or-later",
      architecture: {
        "64bit": {
          url: files.portable.upstream + "#/argus.exe",
          hash: files.portable.sha256,
        },
      },
      bin: "argus.exe",
      shortcuts: [["argus.exe", "Argus"]],
    },
    null,
    2,
  ) + "\n",
);
await put(stage, "release.json", JSON.stringify(release, null, 2) + "\n");

// Only completed builds replace the disposable output beneath this project's target directory.
if (
  output !== resolve(root, "target/official-web/dist") ||
  dirname(stage) !== work
)
  throw Error("Unexpected build path");
const previous = join(work, `previous-${Date.now()}`);
async function moveBuilt(from, to) {
  if (dirname(from) !== work || dirname(to) !== work)
    throw Error("Unexpected build directory");
  for (let attempt = 0; ; attempt++) {
    try {
      await rename(from, to);
      return;
    } catch (error) {
      if (!["EPERM", "EACCES", "EBUSY"].includes(error.code) || attempt === 8)
        throw error;
      await delay(Math.min(1600, 80 * 2 ** attempt));
    }
  }
}
let saved = false;
try {
  await moveBuilt(output, previous);
  saved = true;
} catch (error) {
  if (error.code !== "ENOENT") throw error;
}
try {
  await moveBuilt(stage, output);
} catch (error) {
  if (saved) await moveBuilt(previous, output);
  throw error;
}
if (saved) await rm(previous, { recursive: true, force: true });
console.log(
  `Built ${urls.length} pages from ${tag}, ${docs.length} documents and both verified channels in ${output}`,
);
