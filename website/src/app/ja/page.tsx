import Replica from "@/components/replica/Replica";
import { Home, homeMetadata } from "@/views/Home";
import "@/styles/home-route.css";

export const metadata = homeMetadata("ja");

export default function Page() {
  return <Home lang="ja" demo={<Replica lang="ja" />} />;
}
