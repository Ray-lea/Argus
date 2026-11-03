import { execFileSync } from "node:child_process";
import { readFile, copyFile } from "node:fs/promises";
import { join } from "node:path";
import MarkdownIt from "markdown-it";
import GithubSlugger from "github-slugger";
import sharp from "sharp";
import { put, repoRoot as root, websiteRoot } from "./paths.mjs";
import { origin, repo } from "./release.mjs";

const git = (args) =>
  execFileSync("git", args, { cwd: root, maxBuffer: 32 * 1024 * 1024 });
const fileAt = (ref, file) => git(["show", ref + ":" + file]);
const markdown = new MarkdownIt({ html: false });

function routeFor(file, base) {
  const relative = file
    .slice("docs/".length)
    .replace(/(^|\/)README\.md$/, "$1")
    .replace(/\.md$/, "/");
  return base + relative;
}

async function prepareEdition(input, ref, development, label) {
  const base = development ? "/docs/dev/" : "/docs/";
  const names = git(["ls-tree", "-r", "-z", "--name-only", ref, "--", "docs"])
    .toString()
    .split("\0")
    .filter(Boolean);
  const routes = {};
  for (const file of names.filter(
    (f) => f.endsWith(".md") && !f.endsWith("/AGENTS.md"),
  )) {
    const path = routeFor(file, base);
    const source = fileAt(ref, file).toString("utf8").replace(/\r\n/g, "\n");
    const heading = /^# (.+)$/m.exec(source);
    if (!heading) throw Error("Missing document title in " + file);
    const title = heading[1].trim();
    const body = source.slice(heading.index + heading[0].length).trimStart();
    const lead = markdown
      .parse(body, {})
      .find((t) => t.type === "inline" && t.content.length > 25);
    const summary = lead?.children
      ?.map((t) => t.content)
      .join("")
      .replace(/\s+/g, " ")
      .slice(0, 145);
    const sections = path.split("/").filter(Boolean);
    const breadcrumb = {
      "@context": "https://schema.org",
      "@type": "BreadcrumbList",
      itemListElement: [
        { "@type": "ListItem", position: 1, name: "Argus", item: origin + "/" },
        ...(path === base
          ? []
          : [
              {
                "@type": "ListItem",
                position: 2,
                name: label,
                item: origin + base,
              },
            ]),
        {
          "@type": "ListItem",
          position: path === base ? 2 : 3,
          name: title,
          item: origin + path,
        },
      ],
    };
    const head = [
      { tag: "title", content: title + " · " + label + " · Argus 文档" },
      {
        tag: "script",
        attrs: { type: "application/ld+json" },
        content: JSON.stringify(breadcrumb),
      },
    ];
    const front = [
      "---",
      "title: " + JSON.stringify(title),
      "description: " + JSON.stringify(label + " · " + (summary || title)),
      "head: " + JSON.stringify(head),
      "---",
    ];
    // Starlight owns the visible H1. Preserve the GitHub fragment used by earlier releases.
    const anchor = new GithubSlugger()
      .slug(title)
      .replaceAll("&", "&amp;")
      .replaceAll('"', "&quot;");
    await put(
      input,
      "content/" + sections.join("/") + "/index.md",
      front.join("\n") + '\n\n<span id="' + anchor + '"></span>\n\n' + body,
    );
    routes[file] = path;
  }
  if (!routes["docs/README.md"])
    throw Error("Missing " + label + " documentation index");
  return { ref, label, base, routes };
}

function section(source, heading) {
  const match = source.match(
    new RegExp(
      "^## [^\\n]*" + heading + "[^\\n]*\\n([\\s\\S]*?)(?=^## |$(?![\\s\\S]))",
      "im",
    ),
  );
  if (!match) throw Error("Missing Markdown section: " + heading);
  return match[1].trim();
}
function render(source, rewrite) {
  const tokens = markdown.parse(source, {});
  for (const token of tokens)
    for (const part of token.children ?? []) {
      if (part.type === "link_open")
        part.attrSet("href", rewrite(part.attrGet("href")));
    }
  return markdown.renderer.render(tokens, markdown.options, {});
}

export async function prepareContent(input, release) {
  const devRef = git(["rev-parse", "HEAD"]).toString().trim();
  const editions = [
    await prepareEdition(input, release.tag, false, "稳定版 " + release.tag),
    await prepareEdition(input, devRef, true, "开发版"),
  ];
  const privacy = await readFile(join(root, "PRIVACY.md"), "utf8");
  const translations = {};
  const publicDir = join(input, "public");
  await put(
    publicDir,
    "assets/argus.svg",
    await readFile(join(root, "resources/logo.svg")),
  );
  for (const lang of ["zh", "en"]) {
    const en = lang === "en";
    const prefix = en ? "/en" : "";
    const readme = en ? "README.en.md" : "README.md";
    const current = await readFile(join(root, readme), "utf8");
    const stable = fileAt(release.tag, readme).toString("utf8");
    const requirements = section(
      stable,
      en ? "System requirements" : "系统要求",
    );
    translations[lang] = {
      operatingSystem:
        requirements
          .match(
            /^[-*]\s+\*\*(?:Operating system|操作系统)\*\*\s*[:：]\s*(.+)$/im,
          )?.[1]
          ?.trim() ?? "Windows x64",
      requirements: render(requirements, (href) => {
        const [target, fragment] = href.split("#");
        return editions[0].routes[target]
          ? editions[0].routes[target] + (fragment ? "#" + fragment : "")
          : href;
      }),
      privacy: render(section(privacy, en ? "English" : "简体中文"), (href) =>
        href.startsWith("docs/") ? repo + "/blob/main/" + href : href,
      ),
      signing: render(section(current, "code signing policy"), (href) =>
        href.startsWith("PRIVACY.md") ? prefix + "/privacy/" : href,
      ),
    };
    for (const theme of ["dark", "light"]) {
      const png = fileAt(
        release.tag,
        "docs/images/screenshot-" + (en ? "en-" : "") + theme + ".png",
      );
      const asset = "assets/preview-" + lang + "-" + theme;
      await put(publicDir, asset + ".png", png);
      const { width, height } = await sharp(png).metadata();
      const region = Math.round(width * 0.36);
      const detail = await sharp(png)
        .extract({
          left: 0,
          top: Math.round(height * 0.08),
          width: region,
          height: Math.min(Math.round(region / 1.62), Math.floor(height * 0.9)),
        })
        .resize({ width: 760, withoutEnlargement: true })
        .webp({ quality: 94, nearLossless: true })
        .toBuffer();
      await put(publicDir, asset + "-detail.webp", detail);
      if (theme === "light") {
        const rows = sharp(png).extract({
          left: Math.round(width * 0.016),
          top: Math.round(height * 0.37),
          width: Math.round(width * 0.47),
          height: Math.round(height * 0.24),
        });
        await put(
          publicDir,
          "assets/process-list-" + lang + ".webp",
          await rows.webp({ quality: 94, nearLossless: true }).toBuffer(),
        );
      }
      for (const width of [720, 1280]) {
        await put(
          publicDir,
          asset + "-" + width + ".webp",
          await sharp(png)
            .resize({ width, withoutEnlargement: true })
            .webp({ quality: 92, nearLossless: true })
            .toBuffer(),
        );
      }
    }
  }
  const contact = JSON.parse(
    await readFile(join(websiteRoot, "public-settings.json"), "utf8"),
  ).emailRoutingVerified;
  await put(
    input,
    "site.json",
    JSON.stringify({ release, translations, editions, contact }),
  );
  await copyFile(
    join(websiteRoot, "src/components/magicui/LICENSE"),
    join(publicDir, "assets/magicui-license.txt"),
  );
  const expires = new Date(Date.now() + 350 * 86400000)
    .toISOString()
    .replace(/\.\d{3}Z$/, "Z");
  await put(
    publicDir,
    ".well-known/security.txt",
    "Contact: " +
      repo +
      "/security/advisories/new\n" +
      (contact ? "Contact: mailto:security@argustask.com\n" : "") +
      "Expires: " +
      expires +
      "\nCanonical: " +
      origin +
      "/.well-known/security.txt\nPolicy: " +
      origin +
      "/security/\nPreferred-Languages: zh, en\n",
  );
  await put(
    publicDir,
    "robots.txt",
    "User-agent: *\nAllow: /\nSitemap: " + origin + "/sitemap.xml\n",
  );
  await copyFile(
    join(websiteRoot, "public/_headers"),
    join(publicDir, "_headers"),
  );
}
