import Replica from "@/components/replica/Replica";
import { Home, homeMetadata } from "@/views/Home";
import "@/styles/home-route.css";

export const metadata = homeMetadata("es");

export default function Page() {
  return <Home lang="es" demo={<Replica lang="es" />} />;
}
