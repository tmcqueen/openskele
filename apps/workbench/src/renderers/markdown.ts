// Markdown from agents, rendered as inert DOM: raw HTML is never parsed
// (markdown-it with html off), the result is sanitized, and links open nothing.

import DOMPurify from "dompurify";
import MarkdownIt from "markdown-it";

const md = new MarkdownIt({ html: false, linkify: true, breaks: false });

export function renderMarkdown(source: string): DocumentFragment {
  const fragment = DOMPurify.sanitize(md.render(source), { RETURN_DOM_FRAGMENT: true });
  for (const link of fragment.querySelectorAll("a")) {
    link.setAttribute("rel", "noopener noreferrer");
    // The workbench is not a browser; a link should not replace the app.
    link.addEventListener("click", (event) => event.preventDefault());
  }
  return fragment;
}
