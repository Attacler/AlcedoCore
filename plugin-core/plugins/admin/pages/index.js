// Sample admin plugin page
export default [{
  path: '/sample',
  label: 'Sample Page',
  icon: 'page',
  sidebar: true,
  component: {
    template: `
      <div class="p-6">
        <h1 class="text-2xl font-bold mb-4">Sample Plugin Page</h1>
        <p class="text-gray-600">This is a sample page loaded from the admin plugin's pages directory.</p>
        <div class="mt-4 p-4 bg-gray-100 rounded">
          <p>Route params and query params are accessible to this component.</p>
        </div>
      </div>
    `
  }
}]