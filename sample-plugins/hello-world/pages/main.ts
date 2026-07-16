import IndexPage from './IndexPage.vue'
import CounterPage from './CounterPage.vue'
import ItemsPage from './ItemsPage.vue'
import ExampleView from './ExampleView.vue'
import ExampleCollectionView from './ExampleCollectionView.vue'
import StarRatingInput from './StarRatingInput.vue'
import ColorBadgeDisplay from './ColorBadgeDisplay.vue'

export { IndexPage, CounterPage, ItemsPage, ExampleView, ExampleCollectionView, StarRatingInput, ColorBadgeDisplay }

export default {
  manifestVersion: 1,
  pluginSlug: 'hello-world',
  pages: [
    {
      path: '/',
      label: 'Hello',
      icon: 'home',
      sidebar: true,
      component: IndexPage,
    },
    {
      path: '/counter',
      label: 'Counter',
      icon: 'page',
      sidebar: true,
      component: CounterPage,
    },
    {
      path: '/items',
      label: 'Items',
      icon: 'list',
      sidebar: true,
      component: ItemsPage,
    },
  ],
  views: [
    {
      name: 'example-view',
      label: 'Example View',
      component: ExampleView,
    },
    {
      name: 'example-collection-view',
      label: 'Example Collection View',
      component: ExampleCollectionView,
    },
  ],
  inputs: [
    {
      name: 'star-rating',
      label: 'Star Rating',
      component: StarRatingInput,
    },
  ],
  displays: [
    {
      name: 'color-badge',
      label: 'Color Badge',
      component: ColorBadgeDisplay,
    },
  ],
}
