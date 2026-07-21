import { createRouter, createWebHistory, RouteRecordRaw } from 'vue-router';
import AppLayout from '../components/layout/AppLayout.vue';
import AllItemsView from '../components/views/AllItemsView.vue';
import EmptyPanel from '../components/panel/EmptyPanel.vue';
import PasswordForm from '../components/panel/PasswordForm.vue';
import PasswordDetail from '../components/panel/PasswordDetail.vue';
import FavouritesView from '../components/views/FavouritesView.vue';
import RecentlyAccessedView from '../components/views/RecentlyAccessedView.vue';
import VaultsView from '../components/views/VaultsView.vue';
import VaultOverview from '../components/panel/VaultOverview.vue';
import VaultPasswordsView from '../components/views/VaultPasswordsView.vue';
import VaultForm from '../components/panel/VaultForm.vue';
import { useUser } from '../composables/useUser';
import AuthView from '../components/views/AuthView.vue';

const routes: RouteRecordRaw[] = [
  {
    path: '/',
    component: AppLayout,
    meta: { requiresAuth: true },
    redirect: '/all-items',
    children: [
      {
        path: 'all-items',
        meta: { requiresAuth: true },
        components: {
          list: AllItemsView,
          panel: EmptyPanel
        },
        children: [
          {
            path: 'new',
            components: { panel: PasswordForm }
          },
          {
            path: ':id',
            components: { panel: PasswordDetail }
          },
          {
            path: ':id/edit',
            components: { panel: PasswordForm }
          }
        ]
      },
      {
        path: 'favourites',
        meta: { requiresAuth: true },
        components: {
          list: FavouritesView,
          panel: EmptyPanel
        },
        children: [
          {
            path: ':id',
            components: { panel: PasswordDetail }
          },
          {
            path: ':id/edit',
            components: { panel: PasswordForm }
          }
        ]
      },
      {
        path: 'recently-accessed',
        meta: { requiresAuth: true },
        components: {
          list: RecentlyAccessedView,
          panel: EmptyPanel
        },
        children: [
          {
            path: ':id',
            components: { panel: PasswordDetail }
          },
          {
            path: ':id/edit',
            components: { panel: PasswordForm }
          }
        ]
      },
      {
        path: 'vaults',
        meta: { requiresAuth: true },
        children: [
          {
            path: 'new',
            components: { panel: VaultForm }
          },
          {
            path: 'edit/:id',
            components: { panel: VaultForm }
          },
          {
            path: ':vaultId',
            components: {
              list: VaultPasswordsView,
              panel: EmptyPanel
            },
            children: [
              {
                path: 'new',
                components: { panel: PasswordForm }
              },
              {
                path: ':id',
                components: { panel: PasswordDetail }
              },
              {
                path: ':id/edit',
                components: { panel: PasswordForm }
              }
            ]
          }
        ]
      }
    ]
  },
  {
    path: '/auth',
    component: AuthView
  },

  /* 404 fallback */
  {
    path: '/:pathMatch(.*)*',
    redirect: '/auth'
  }
];

const router = createRouter({
  history: createWebHistory(),
  routes
});

router.beforeEach((to, _from) => {
  const { user } = useUser();

  if (to.meta?.requiresAuth && !user.value) {
    return '/auth';
  }
});

export default router;
