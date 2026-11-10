import { Download, downloadMetadata } from "@/views/Download";
import "@/styles/pages-route.css";

export const metadata = downloadMetadata("fr");

export default function Page() {
  return <Download lang="fr" />;
}
