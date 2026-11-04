"use client";
import NextLink from "next/link";
import { useRouter } from "next/navigation";
import type { AnchorHTMLAttributes, ComponentProps, FocusEvent, MouseEvent, TouchEvent } from "react";

// A page is fetched ahead when a visitor points at, focuses or touches its
// link, not whenever the link scrolls into view: the documentation's sidebar
// alone would otherwise fetch every page it lists as soon as it shows.
export function IntentLink({ prefetch, onMouseEnter, onFocus, onTouchStart, ...props }: ComponentProps<typeof NextLink>) {
  const router = useRouter();
  const intent = () => {
    if (prefetch !== false && typeof props.href === "string") router.prefetch(props.href);
  };
  return (
    <NextLink
      {...props}
      prefetch={false}
      onMouseEnter={(event: MouseEvent<HTMLAnchorElement>) => {
        onMouseEnter?.(event);
        intent();
      }}
      onFocus={(event: FocusEvent<HTMLAnchorElement>) => {
        onFocus?.(event);
        intent();
      }}
      onTouchStart={(event: TouchEvent<HTMLAnchorElement>) => {
        onTouchStart?.(event);
        intent();
      }}
    />
  );
}

// The same for links inside Fumadocs, which hands over a plain anchor's props.
export function DocsLink({ href = "#", ...props }: AnchorHTMLAttributes<HTMLAnchorElement> & { prefetch?: boolean }) {
  return <IntentLink href={href} {...props} />;
}
