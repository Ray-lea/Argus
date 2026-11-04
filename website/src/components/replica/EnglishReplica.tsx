"use client";
import { exact, patterns } from "./i18n/en";
import Replica from "./Replica";

// The English home page's window: the same demo with its English words, which
// load with this page alone.
const english = { exact, patterns };
export default function EnglishReplica() {
  return <Replica lang="en" dictionary={english} />;
}
