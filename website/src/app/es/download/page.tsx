import { Download, downloadMetadata } from "@/views/Download";
import "@/styles/pages-route.css";

export const metadata = downloadMetadata("es");

export default function Page() {
  return <Download lang="es" />;
}
