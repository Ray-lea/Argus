const input = document.querySelector("#query");
const term = (new URL(location.href).searchParams.get("q") ?? "")
  .trim()
  .slice(0, 160);
input.value = term;
const status = document.querySelector("#status");
if (term) {
  fetch("/assets/search-index.json")
    .then((response) => {
      if (!response.ok) throw Error();
      return response.json();
    })
    .then((pages) => {
      const terms = term.toLocaleLowerCase().split(/\s+/);
      const hits = pages
        .filter((page) =>
          terms.every((t) => page.text.toLocaleLowerCase().includes(t)),
        )
        .sort(
          (a, b) =>
            Number(terms.some((t) => b.title.toLocaleLowerCase().includes(t))) -
            Number(terms.some((t) => a.title.toLocaleLowerCase().includes(t))),
        );
      status.textContent = hits.length
        ? `找到 ${hits.length} 篇文档`
        : "未找到匹配的文档";
      const list = document.querySelector("#results");
      for (const page of hits) {
        const li = document.createElement("li"),
          link = document.createElement("a"),
          excerpt = document.createElement("p");
        link.href = page.path;
        link.textContent = page.title;
        const index = page.text.toLocaleLowerCase().indexOf(terms[0]);
        excerpt.textContent = page.text.slice(
          Math.max(0, index - 22),
          index + 85,
        );
        li.append(link, excerpt);
        list.append(li);
      }
    })
    .catch(() => {
      status.textContent = "搜索索引暂时不可用，请从文档目录浏览。";
    });
}
