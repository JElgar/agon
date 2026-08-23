# Agon UI

A modern React 19 application with Tailwind CSS, shadcn/ui, PWA support, and Supabase authentication.

## Features

- ⚛️ React 19 with TypeScript
- 🎨 Tailwind CSS for styling
- 📱 PWA support (installable app)
- 🔐 Supabase authentication with Google OAuth
- 🧩 shadcn/ui component library
- 🔌 Auto-generated TypeScript API client
- ⚡ Vite for fast development

## Setup

1. Clone and install dependencies:
```bash
cd agon_ui
npm install
```

2. Configure environment variables:
```bash
cp .env.example .env
```

Edit `.env` and add your configuration:
```
VITE_SUPABASE_URL=your_supabase_project_url
VITE_SUPABASE_ANON_KEY=your_supabase_anon_key
```

3. Start development server:
```bash
npm run dev
```

## PWA Installation

When deployed, users can install the app on their devices:
- **Desktop**: Click the install button in the browser address bar
- **Mobile**: Use "Add to Home Screen" option in the browser menu

## Development Commands

- `npm run dev` - Start development server
- `npm run build` - Build for production
- `npm run preview` - Preview production build
- `npm run lint` - Run ESLint

## Project Structure

```
src/
├── api/               # Auto-generated TypeScript API client
├── components/
│   ├── auth/          # Authentication components
│   └── ui/            # shadcn/ui components
├── hooks/             # Custom React hooks
│   ├── useAuth.tsx    # Authentication state management
│   └── useApi.ts      # API client hooks
├── lib/               # Utility functions and configurations
│   ├── api.ts         # Configured API client
│   ├── supabase.ts    # Supabase client
│   └── utils.ts       # General utilities
└── App.tsx            # Main application component
```

## Authentication

The app uses Supabase for authentication with the following features:
- **Google OAuth**: One-click sign in with Google
- **Email/password**: Traditional authentication method
- **Automatic session management**: JWT tokens handled automatically
- **Protected routes**: All API calls authenticated with user tokens
- **Sign out functionality**: Clean session termination

## API Client

The app includes a fully typed TypeScript client generated from the agon_service OpenAPI spec:

### Available API Operations:
- `api.createUser(input)` - Create a new user
- `api.createTeam(input)` - Create a new team
- `api.getTeams()` - List user's teams
- `api.getTeam(id)` - Get team details
- `api.addTeamMembers(teamId, input)` - Add members to team

### Using API Hooks:
```typescript
import { useGetTeams, useCreateTeam } from '@/hooks/useApi'

function MyComponent() {
  const { data: teams, loading, error, getTeams } = useGetTeams()
  const { createTeam } = useCreateTeam()
  
  const handleCreate = () => createTeam({ name: "New Team" })
  
  // Component logic...
}
```

### Regenerating API Client:
```bash
# From the root agon directory
make generate-schema
cd agon_ui
npx openapi-typescript-codegen --input ../schema.json --output ./src/api --client axios
```

## Testing

Full browser end-to-end tests live in `e2e/` (Playwright) — see
`e2e/README.md` for what they cover and how to run them. They're separate
from `agon_tests` at the repo root, which tests the API directly and doesn't
touch the browser.

## Adding shadcn/ui Components

To add more shadcn/ui components:

```bash
npx shadcn@latest add [component-name]
```

Note: React 19 may require `--force` flag for some installations due to peer dependency issues.
