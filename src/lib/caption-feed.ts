import type { CaptionTranslatedEvent } from "./contracts";

/** Revisions update a row in place and never move an old subtitle to the end. */
export function acceptTranslation(items: CaptionTranslatedEvent[], incoming: CaptionTranslatedEvent) {
  const index = items.findIndex(item => item.segment.id === incoming.segment.id);
  if (index < 0) return [...items, incoming].slice(-12);
  if (items[index].segment.revision >= incoming.segment.revision) return items;
  return items.map((item, i) => i === index ? incoming : item);
}
