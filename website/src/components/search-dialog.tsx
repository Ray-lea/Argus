"use client";
import { useStaticSearch } from "fumadocs-core/search/client";
import {
  SearchDialog,
  SearchDialogClose,
  SearchDialogContent,
  SearchDialogFooter,
  SearchDialogHeader,
  SearchDialogIcon,
  SearchDialogInput,
  SearchDialogList,
  SearchDialogOverlay,
  TagsList,
  TagsListItem,
  type SharedProps,
} from "fumadocs-ui/components/dialog/search";
import { usePathname } from "next/navigation";
import { useEffect, useState } from "react";
import { create } from "zbsearch";
import { useSiteCopy } from "./site/LocaleProvider";

// The index is built with Intl.Segmenter word breaks; queries must be split the same way.
const initDB = () => create({ schema: { _: "string" }, language: "multilingual" });

export default function DocsSearch(props: SharedProps) {
  const words = useSiteCopy({ zh: { search: "搜索文档，例如 特权服务、快捷键、显存", stable: "稳定版", dev: "开发版", select: "选择", open: "打开" }, en: { search: "Search documentation in Simplified Chinese", stable: "Stable", dev: "Development", select: "Select", open: "Open" } });
  const path = usePathname();
  const current = path === "/docs/dev" || path.startsWith("/docs/dev/") ? "dev" : "stable";
  const [edition, setEdition] = useState<string | undefined>(current);
  // Each time the dialog opens it searches the edition being read.
  useEffect(() => {
    if (props.open) setEdition(current);
  }, [props.open, current]);
  const search = useStaticSearch({ from: "/docs-search.json", tag: edition, initDB });
  return (
    <SearchDialog {...search} {...props}>
      <SearchDialogOverlay />
      <SearchDialogContent>
        <SearchDialogHeader>
          <SearchDialogIcon />
          <SearchDialogInput placeholder={words.search} />
          <SearchDialogClose />
        </SearchDialogHeader>
        <SearchDialogList />
        <SearchDialogFooter className="flex flex-row items-center gap-2">
          <TagsList tag={edition} onTagChange={setEdition} allowClear>
            <TagsListItem value="stable">{words.stable}</TagsListItem>
            <TagsListItem value="dev">{words.dev}</TagsListItem>
          </TagsList>
          <span className="ms-auto text-xs text-fd-muted-foreground">
            <kbd>↑</kbd> <kbd>↓</kbd> {words.select} · <kbd>Enter</kbd> {words.open}
          </span>
        </SearchDialogFooter>
      </SearchDialogContent>
    </SearchDialog>
  );
}
