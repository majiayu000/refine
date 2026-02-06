/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{js,ts,jsx,tsx}'],
  theme: {
    extend: {
      colors: {
        brand: {
          50: '#f0fdfa',
          100: '#ccfbf1',
          200: '#99f6e4',
          300: '#5eead4',
          400: '#2dd4bf',
          500: '#14b8a6',
          600: '#0d9488',
          700: '#0f766e',
          800: '#115e59',
          900: '#134e4a',
        },
        sand: {
          50: '#fcfaf4',
          100: '#f8f3e8',
          200: '#efe4cf',
          300: '#dec9a8',
          400: '#c8a97f',
          500: '#af885f',
          600: '#916b4a',
          700: '#73533b',
          800: '#5f4535',
          900: '#4f3a2e',
        },
      },
      boxShadow: {
        soft: '0 18px 40px -24px rgba(15, 23, 42, 0.45)',
        glow: '0 24px 42px -26px rgba(13, 148, 136, 0.5)',
      },
      keyframes: {
        'rise-in': {
          '0%': { opacity: 0, transform: 'translateY(10px) scale(0.985)' },
          '100%': { opacity: 1, transform: 'translateY(0) scale(1)' },
        },
        'pulse-soft': {
          '0%, 100%': { opacity: 0.55 },
          '50%': { opacity: 1 },
        },
      },
      animation: {
        'rise-in': 'rise-in 420ms cubic-bezier(0.2, 0.8, 0.2, 1) both',
        'pulse-soft': 'pulse-soft 2.8s ease-in-out infinite',
      },
      fontFamily: {
        sans: ['Space Grotesk', 'Avenir Next', 'PingFang SC', 'sans-serif'],
        display: ['Space Grotesk', 'Avenir Next', 'PingFang SC', 'sans-serif'],
        mono: ['IBM Plex Mono', 'JetBrains Mono', 'monospace'],
      },
    },
  },
  plugins: [],
}
