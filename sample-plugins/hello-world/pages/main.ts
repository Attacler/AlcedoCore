import IndexPage from "./IndexPage.vue";
import CounterPage from "./CounterPage.vue";
import ItemsPage from "./ItemsPage.vue";
import ExampleView from "./ExampleView.vue";
import ExampleCollectionView from "./ExampleCollectionView.vue";
import StarRatingInput from "./StarRatingInput.vue";
import StarRatingDisplay from "./StarRatingDisplay.vue";
import TestDisplay from "./TestDisplay.vue";
import TestDisplaySettings from "./TestDisplaySettings.vue";

export default {
    manifestVersion: 2,
    pluginSlug: "hello-world",
    pages: [
        {
            path: "/",
            label: "Hello",
            icon: "home",
            sidebar: true,
            component: IndexPage,
        },
        {
            path: "/counter",
            label: "Counter",
            icon: "calculate",
            sidebar: true,
            component: CounterPage,
        },
        {
            path: "/items",
            label: "Items",
            icon: "database",
            sidebar: true,
            component: ItemsPage,
        },
    ],
    views: [
        {
            name: "example-view",
            label: "Example View",
            icon: "view_apps",
            component: ExampleView,
        },
        {
            name: "example-collection-view",
            label: "Example Collection View",
            icon: "view_apps",
            component: ExampleCollectionView,
        },
    ],
    inputs: [
        {
            name: "star-rating",
            label: "Star Rating",
            icon: "star_half",
            component: StarRatingInput,
            group: "Number",
            supportedFieldTypes: ["int"],
        },
    ],
    displays: [
        {
            name: "star-rating",
            label: "Star Rating",
            icon: "star_half",
            component: StarRatingDisplay,
            supportedFieldTypes: ["int"],
            preferredInputs: ["star-rating", "number"],
        },
        {
            name: "test-display",
            label: "Test Display",
            group: "Dev Test",
            icon: "box",
            component: TestDisplay,
            settingsComponent: TestDisplaySettings,
            supportedFieldTypes: ["int"],
            preferredInputs: ["number"],
        },
    ],
};
