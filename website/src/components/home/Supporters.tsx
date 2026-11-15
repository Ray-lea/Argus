import type { CSSProperties } from "react";
import supporters from "@/data/supporters.json";
import { localize } from "@/lib/site-i18n";
import type { Language } from "@/lib/site";

// Project support and special thanks: one card per supporter, the last cells
// of the open-source grid. A field of light in the colours of their avatar
// drifts behind the portrait; the README cards are drawn from the same list
// (src/data/README.md, scripts/supporters.mjs). Links open in a new tab.
export function Supporters({ lang }: { lang: Language }) {
  const en = lang !== "zh";
  const tier = localize(lang, en ? "Project support & special thanks" : "项目支持 & 特别致谢");
  const thanks = localize(lang, en ? "Argus keeps moving forward thanks to this support." : "Argus 的持续开发得益于这份支持，谨致谢忱。");
  return supporters.map(({ id, name, url, colors, signature }) => {
    const style = Object.fromEntries([
      ...colors.light.map((colour, i) => [`--l${i}`, colour]),
      ...colors.dark.map((colour, i) => [`--d${i}`, colour]),
    ]) as CSSProperties;
    return (
      <article key={id} className="cell cell-thanks" data-spotlight="" data-motion="" data-rise="" style={style}>
        <div className="thanks-sky" aria-hidden="true">
          {[0, 1, 2, 3, 4].map((i) => (
            <i key={i} className={`thanks-light thanks-light-${i}`} />
          ))}
        </div>
        <div className="thanks-grain" aria-hidden="true" />
        <a className="thanks-go" href={url} target="_blank" rel="noopener noreferrer" tabIndex={-1} aria-hidden="true">
          <svg viewBox="0 0 16 16">
            <path d="M4.5 11.5l7-7M6 4.5h5.5V10" />
          </svg>
        </a>
        <div className="thanks-body">
          <span className="thanks-avatar">
            <img src={`/assets/supporters/${id}.webp`} alt="" width={124} height={124} loading="lazy" decoding="async" />
          </span>
          <div className="thanks-text">
            <span className="thanks-chip">
              <img src="/assets/argus.svg" alt="" width={15} height={15} />
              <b>Argus</b>
              <i aria-hidden="true" />
              {tier}
            </span>
            <a className="thanks-name" href={url} target="_blank" rel="noopener noreferrer">
              {signature ? (
                <>
                  {/* The nickname in Great Vibes, as outlines drawn at 100 px:
                      the same letters as the README card, with no font to load. */}
                  <svg
                    className="thanks-signature"
                    viewBox={`${signature.left} ${-signature.top} ${signature.right - signature.left} ${signature.top + signature.bottom}`}
                    width={`${(signature.right - signature.left) / 100}em`}
                    height={`${(signature.top + signature.bottom) / 100}em`}
                    aria-hidden="true"
                  >
                    <path d={signature.d} />
                  </svg>
                  <span className="sr-only">{name}</span>
                </>
              ) : (
                name
              )}
            </a>
            <p className="thanks-note">{thanks}</p>
          </div>
        </div>
      </article>
    );
  });
}
