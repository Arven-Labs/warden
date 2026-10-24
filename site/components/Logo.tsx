/** The Warden shield-and-W mark; the gradient id is per instance-safe because every copy is identical. */
export function LogoMark({ size = 28 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" fill="none" aria-hidden="true">
      <defs>
        <linearGradient id="warden-mark" x1="6" y1="2" x2="26" y2="30" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="#c4b5fd" />
          <stop offset="1" stopColor="#7c3aed" />
        </linearGradient>
      </defs>
      <path
        d="M16 2.5 27 6.2v8.6c0 6.4-4.4 11.1-11 14.2C9.4 25.9 5 21.2 5 14.8V6.2Z"
        fill="#0d0a18"
        stroke="url(#warden-mark)"
        strokeWidth="1.8"
        strokeLinejoin="round"
      />
      <path
        d="m10 11.2 3.1 9 2.9-6.2 2.9 6.2 3.1-9"
        stroke="url(#warden-mark)"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

export function Logo() {
  return (
    <span className="logo">
      <LogoMark />
      <span>Warden</span>
    </span>
  );
}
