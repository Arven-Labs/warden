/** Warden's mark: a shield with a W, stroked in the purple gradient on near-black (the same drawing as `desktop/src/components/Icons.tsx`
 * and `site/public/brand/icon.svg`). */
export default function LogoMark({ size = 22 }: { size?: number }) {
  const gradientId = `warden-logo-gradient-${size}`;
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" fill="none" aria-hidden="true">
      <defs>
        <linearGradient id={gradientId} x1="6" y1="2" x2="26" y2="30" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="#c4b5fd" />
          <stop offset="1" stopColor="#7c3aed" />
        </linearGradient>
      </defs>
      <path
        d="M16 2.5 27 6.2v8.6c0 6.4-4.4 11.1-11 14.2C9.4 25.9 5 21.2 5 14.8V6.2Z"
        fill="#0d0a18"
        stroke={`url(#${gradientId})`}
        strokeWidth="1.8"
        strokeLinejoin="round"
      />
      <path d="m10 11.2 3.1 9 2.9-6.2 2.9 6.2 3.1-9" stroke={`url(#${gradientId})`} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}
