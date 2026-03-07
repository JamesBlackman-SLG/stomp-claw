import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { RouterProvider, createRouter, createRootRoute, createRoute } from '@tanstack/react-router'
import { AppProvider } from './lib/state'
import { Home } from './routes/index'
import './styles/app.css'

const rootRoute = createRootRoute({
  component: () => (
    <AppProvider>
      <div className="h-screen flex flex-col bg-bg text-text font-mono">
        <Home />
      </div>
    </AppProvider>
  ),
})

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/',
  component: Home,
})

const routeTree = rootRoute.addChildren([indexRoute])
const router = createRouter({ routeTree })

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <RouterProvider router={router} />
  </StrictMode>,
)
