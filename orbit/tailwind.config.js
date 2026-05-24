/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        bg: '#07090d',
        surface: '#0c1117',
        'surface-2': '#11171f',
        'surface-3': '#161d27',
        border: '#1c2330',
        'border-bright': '#2a3344',
        'border-hot': '#3d4a63',
        text: '#e8eef5',
        dim: '#8a95a7',
        dimmer: '#525c6f',
        faint: '#2f3848',
        amber: '#ffb454',
        'amber-hot': '#ffc97a',
        cyan: '#56d3ff',
        'cyan-hot': '#8ce4ff',
        green: '#7ff19f',
        red: '#ff5c5c',
        magenta: '#ff8eb6',
      },
      fontFamily: {
        mono: [
          '"JetBrains Mono"',
          '"Berkeley Mono"',
          'ui-monospace',
          'SFMono-Regular',
          'Menlo',
          'monospace',
        ],
      },
      letterSpacing: {
        widest: '0.22em',
        wider: '0.14em',
      },
      boxShadow: {
        'glow-amber': '0 0 16px -2px rgba(255, 180, 84, 0.45)',
        'glow-cyan': '0 0 16px -2px rgba(86, 211, 255, 0.4)',
        'glow-green': '0 0 14px -2px rgba(127, 241, 159, 0.45)',
        'glow-red': '0 0 14px -2px rgba(255, 92, 92, 0.5)',
        'inset-border': 'inset 0 0 0 1px rgba(60, 75, 100, 0.35)',
      },
      animation: {
        'pulse-soft': 'pulse-soft 2.4s ease-in-out infinite',
        scan: 'scan 6s linear infinite',
        'fade-in': 'fade-in 0.5s ease-out both',
        'slide-up': 'slide-up 0.45s cubic-bezier(0.22, 1, 0.36, 1) both',
        'cursor-blink': 'cursor-blink 1.1s steps(2) infinite',
        spin12s: 'spin 12s linear infinite',
      },
      keyframes: {
        'pulse-soft': {
          '0%, 100%': { opacity: '1' },
          '50%': { opacity: '0.35' },
        },
        scan: {
          '0%': { transform: 'translateY(-120%)' },
          '100%': { transform: 'translateY(220%)' },
        },
        'fade-in': {
          from: { opacity: '0' },
          to: { opacity: '1' },
        },
        'slide-up': {
          from: { opacity: '0', transform: 'translateY(8px)' },
          to: { opacity: '1', transform: 'translateY(0)' },
        },
        'cursor-blink': {
          '0%, 50%': { opacity: '1' },
          '51%, 100%': { opacity: '0' },
        },
      },
    },
  },
  plugins: [],
}
