// Academy art (assets/academy/*.webp, generated for Zorvik): unit illustrations and badges.
const images = import.meta.glob("../../assets/academy/*.webp", { eager: true, query: "?url", import: "default" }) as Record<string, string>;

export const unitImage = (name: string): string | undefined => images[`../../assets/academy/unit-${name}.webp`];
export const badgeImage = (id: string): string | undefined => images[`../../assets/academy/badge-${id}.webp`];
