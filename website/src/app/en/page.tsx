import EnglishReplica from "@/components/replica/EnglishReplica";
import { Home, homeMetadata } from "@/views/Home";
import "@/styles/home-route.css";

export const metadata = homeMetadata("en");

export default function Page() {
  return <Home lang="en" demo={<EnglishReplica />} />;
}
