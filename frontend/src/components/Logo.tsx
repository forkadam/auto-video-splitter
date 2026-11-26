import appIcon from "@/assets/app-icon.png"

type Props = { className?: string }

/** Sleek vsplit brand mark matching the native desktop app icon. */
export function Logo({ className }: Props) {
  return (
    <img
      src={appIcon}
      alt="vsplit logo"
      className={className}
      draggable={false}
    />
  )
}
