import Replica from "@/components/replica/Replica";
import { Home, homeMetadata } from "@/views/Home";
import "@/styles/home-route.css";

export const metadata = homeMetadata("fr");

export default function Page() {
  return <Home lang="fr" demo={<Replica lang="fr" />} />;
}
