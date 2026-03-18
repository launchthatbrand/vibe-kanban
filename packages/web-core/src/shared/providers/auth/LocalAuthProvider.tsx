import { useMemo, type ReactNode } from 'react';
import {
  AuthContext,
  type AuthContextValue,
} from '@/shared/hooks/auth/useAuth';
import { useUserSystem } from '@/shared/hooks/useUserSystem';

interface LocalAuthProviderProps {
  children: ReactNode;
}

export function LocalAuthProvider({ children }: LocalAuthProviderProps) {
  const { loginStatus } = useUserSystem();
  const bypassAuth =
    import.meta.env.VITE_BYPASS_AUTH === 'true' ||
    import.meta.env.VITE_BYPASS_AUTH === '1';

  if (import.meta.env.DEV && bypassAuth) {
    console.info('[auth] VITE_BYPASS_AUTH is enabled for local development');
  }

  const value = useMemo<AuthContextValue>(
    () => ({
      isSignedIn: bypassAuth || loginStatus?.status === 'loggedin',
      isLoaded: loginStatus !== null,
      // Provide a stable local user id when bypassing auth in development.
      userId:
        bypassAuth
          ? 'local-dev-bypass-user'
          : loginStatus?.status === 'loggedin'
            ? loginStatus.profile.user_id
            : null,
    }),
    [bypassAuth, loginStatus]
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}
