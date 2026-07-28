import { Command } from "commander";
import path from "node:path";
import fs from "node:fs";
import crypto from "node:crypto";
import { execSync } from "node:child_process";
import detectPort from "detect-port";
import checkDiskSpaceModule from "check-disk-space";
import {
    createSpinner,
    success,
    error as logError,
    info,
    warn,
} from "../utils/logger";

interface CoreConfig {
    platform: "docker" | "k8s";
    adminEmail: string;
    adminPassword: string;
    dbPassword: string;
    corePort: number;
    swarmEnabled: boolean;
    coreImageTag: string;
    postgresVersion: string;
    redisPassword: string;
    sessionTtl: number;
    rateLimitRequests: number;
    rateLimitWindow: number;
    corsOrigins: string;
    enableCors: boolean;
    enableRateLimit: boolean;
    corePublicUrl: string;
    registryPort: number;
    systemPluginsUrl: string;
    localRegistryUrl: string;
    pluginNetwork: string;
    registryEncryptionKey: string;
}

export const initCoreCommand = new Command("init-core")
    .description(
        "Generate configuration for running Alcedo Core (Docker Compose or Kubernetes)",
    )
    .option("-o, --output-dir <path>", "Output directory (default: ./alcedo)")
    .option(
        "-P, --platform <type>",
        "Platform: docker or k8s (default: docker, prompts if interactive)",
    )
    .option(
        "-e, --admin-email <email>",
        "Admin email (skips prompt if provided)",
    )
    .option(
        "-p, --admin-password <password>",
        "Admin password (skips prompt if provided)",
    )
    .option("--port <port>", "Core HTTP port", parseInt)
    .option("--non-interactive", "Skip prompts, use defaults", false)
    .action(
        async (options: {
            outputDir?: string;
            platform?: string;
            adminEmail?: string;
            adminPassword?: string;
            port?: number;
            nonInteractive: boolean;
        }) => {
            const targetDir = path.resolve(options.outputDir || "./alcedo");

            if (
                fs.existsSync(targetDir) &&
                fs.readdirSync(targetDir).length > 0
            ) {
                logError(
                    `Directory already exists and is not empty: ${targetDir}`,
                );
                process.exit(1);
            }

            const defaults: CoreConfig = {
                platform: (options.platform as "docker" | "k8s") || "docker",
                adminEmail: options.adminEmail || "admin@alcedo.dev",
                adminPassword: options.adminPassword || generatePassword(),
                dbPassword: generatePassword(),
                corePort: options.port || 8080,
                swarmEnabled: false,
                coreImageTag: "alcedocore/core:0.1.0",
                postgresVersion: "16",
                redisPassword: "",
                sessionTtl: 86400,
                rateLimitRequests: 200,
                rateLimitWindow: 60,
                corsOrigins: "*",
                enableCors: true,
                enableRateLimit: true,
                corePublicUrl: "",
                registryPort: 5000,
                systemPluginsUrl: "https://cspm.alcedocore.nl",
                localRegistryUrl: "localhost:5000",
                pluginNetwork: "alcedo_plugins",
                registryEncryptionKey: generateRegistryKey(),
            };

            // Pre-flight checks (Docker only)
            if (
                defaults.platform === "docker" &&
                !(await preflightCheck(defaults))
            ) {
                logError(
                    "Pre-flight checks failed. Please fix the issues above and try again.",
                );
                process.exit(1);
            }

            try {
                const config = options.nonInteractive
                    ? defaults
                    : await prompt(defaults);

                await writeFiles(targetDir, config);
            } catch (err: any) {
                logError(`Failed to generate configuration: ${err.message}`);
                process.exit(1);
            }
        },
    );

function generatePassword(length = 24): string {
    return crypto.randomBytes(length).toString("base64url").slice(0, length);
}

function generateRegistryKey(): string {
    return crypto.randomBytes(32).toString("base64");
}

function checkDocker(): boolean {
    try {
        execSync("docker info --format '{{.ServerVersion}}'", {
            stdio: "pipe",
            timeout: 5000,
        });
        return true;
    } catch {
        return false;
    }
}

async function checkPort(port: number): Promise<boolean> {
    try {
        const actual = await detectPort(port);
        return actual === port; // if detectPort returns same port, it's free
    } catch {
        return true; // on error, assume free
    }
}

async function checkDiskSpaceFree(
    minGB: number = 1,
): Promise<{ ok: boolean; freeGB: number }> {
    try {
        const disk = await checkDiskSpaceModule("/");
        const freeGB = Math.floor(disk.free / 1024 / 1024 / 1024);
        return { ok: freeGB >= minGB, freeGB };
    } catch {
        return { ok: true, freeGB: 99 };
    }
}

async function preflightCheck(config: CoreConfig): Promise<boolean> {
    let ok = true;

    if (!checkDocker()) {
        logError("Docker is not installed or the daemon is not running.");
        info("  Install Docker: https://docs.docker.com/engine/install/");
        ok = false;
    }

    const disk = await checkDiskSpaceFree();
    if (!disk.ok) {
        warn(
            `Low disk space: only ${disk.freeGB}GB available (minimum 1GB recommended).`,
        );
    }

    if (!(await checkPort(config.corePort))) {
        warn(
            `Port ${config.corePort} is already in use. The core may fail to start.`,
        );
    }
    if (!(await checkPort(config.registryPort))) {
        warn(
            `Port ${config.registryPort} is already in use. The registry may fail to start.`,
        );
    }

    return ok;
}

function detectSwarmState(): "active" | "inactive" | "disabled" {
    try {
        const out = execSync(
            "docker info --format '{{.Swarm.LocalNodeState}}'",
            { encoding: "utf-8", timeout: 5000 },
        ).trim();
        if (out === "active") return "active";
        if (out === "inactive") return "inactive";
        return "disabled";
    } catch {
        return "disabled";
    }
}

async function prompt(config: CoreConfig): Promise<CoreConfig> {
    const inquirer = (await import("inquirer")).default;

    const DEFAULT_PLUGINS_URL = "https://cspm.alcedocore.nl";

    // Platform selection
    const platformAnswer = await inquirer.prompt<{ platform: string }>([
        {
            type: "list",
            name: "platform",
            message: "Which platform?",
            choices: [
                {
                    name: "Docker Compose (local dev / single host)",
                    value: "docker",
                },
                { name: "Kubernetes (K8s cluster)", value: "k8s" },
            ],
            default: config.platform,
        },
    ]);
    config.platform = platformAnswer.platform as "docker" | "k8s";

    if (config.platform === "docker") {
        // Detect Swarm state automatically
        const swarmState = detectSwarmState();
        if (swarmState === "active") {
            config.swarmEnabled = true;
            info("Docker Swarm is active — Swarm support enabled.");
        } else if (swarmState === "inactive") {
            const initSwarm = await inquirer.prompt<{ confirm: boolean }>([
                {
                    type: "confirm",
                    name: "confirm",
                    message:
                        "Docker Swarm is not initialized. Initialize it now?",
                    default: false,
                },
            ]);
            if (initSwarm.confirm) {
                try {
                    execSync("docker swarm init", {
                        stdio: "ignore",
                        timeout: 10000,
                    });
                    config.swarmEnabled = true;
                    info("Docker Swarm initialized.");
                } catch {
                    info(
                        "Failed to initialize Swarm — proceeding without Swarm support.",
                    );
                    config.swarmEnabled = false;
                }
            } else {
                config.swarmEnabled = false;
            }
        } else {
            info(
                "Docker Swarm is not available on this host — Swarm support disabled.",
            );
            config.swarmEnabled = false;
        }
    }

    const basic = await inquirer.prompt<{
        adminEmail: string;
        adminPassword: string;
        corePort: number;
        corePublicUrl: string;
        useCustomPluginsUrl: boolean;
        advanced: boolean;
    }>([
        {
            type: "input",
            name: "adminEmail",
            message: "Admin email:",
            default: config.adminEmail,
            validate: (v: string) =>
                v.includes("@") || "Please enter a valid email",
        },
        {
            type: "password",
            name: "adminPassword",
            message: "Admin password:",
            default: config.adminPassword,
            validate: (v: string) =>
                v.length >= 8 || "Password must be at least 8 characters",
        },
        {
            type: "number",
            name: "corePort",
            message: "Core HTTP port:",
            default: config.corePort,
        },
        {
            type: "input",
            name: "corePublicUrl",
            message:
                "Core public URL (leave empty to use http://localhost:<port>):",
            default: config.corePublicUrl,
        },
        {
            type: "confirm",
            name: "useCustomPluginsUrl",
            message: "Do you want to use a custom system plugins manifest URL?",
            default: false,
        },
        {
            type: "confirm",
            name: "advanced",
            message: "Configure advanced options?",
            default: false,
        },
    ]);

    config.adminEmail = basic.adminEmail;
    config.adminPassword = basic.adminPassword;
    config.corePort = basic.corePort;
    config.corePublicUrl = basic.corePublicUrl;

    if (basic.useCustomPluginsUrl) {
        const custom = await inquirer.prompt<{ systemPluginsUrl: string }>([
            {
                type: "input",
                name: "systemPluginsUrl",
                message: "Custom system plugins manifest URL:",
                default: config.systemPluginsUrl,
            },
        ]);
        config.systemPluginsUrl = custom.systemPluginsUrl;
    } else {
        config.systemPluginsUrl = DEFAULT_PLUGINS_URL;
    }

    if (basic.advanced) {
        const advanced = await inquirer.prompt<{
            dbPassword: string;
            redisPassword: string;
            coreImageTag: string;
            postgresVersion: string;
            sessionTtl: number;
            enableCors: boolean;
            corsOrigins: string;
            enableRateLimit: boolean;
            rateLimitRequests: number;
            rateLimitWindow: number;
            pluginNetwork: string;
            registryPort: number;
            localRegistryUrl: string;
        }>([
            {
                type: "input",
                name: "dbPassword",
                message: "PostgreSQL password:",
                default: config.dbPassword,
            },
            {
                type: "input",
                name: "redisPassword",
                message: "Redis password (leave empty for none):",
                default: config.redisPassword,
            },
            {
                type: "input",
                name: "coreImageTag",
                message: "Core Docker image tag:",
                default: config.coreImageTag,
            },
            {
                type: "input",
                name: "postgresVersion",
                message: "PostgreSQL version:",
                default: config.postgresVersion,
            },
            {
                type: "number",
                name: "sessionTtl",
                message: "Session TTL (seconds):",
                default: config.sessionTtl,
            },
            {
                type: "confirm",
                name: "enableCors",
                message: "Enable CORS?",
                default: config.enableCors,
            },
            {
                type: "input",
                name: "corsOrigins",
                message: "Allowed CORS origins (comma-separated or *):",
                default: config.corsOrigins,
                when: (a: any) => a.enableCors,
            },
            {
                type: "confirm",
                name: "enableRateLimit",
                message: "Enable rate limiting?",
                default: config.enableRateLimit,
            },
            {
                type: "number",
                name: "rateLimitRequests",
                message: "Max requests per window:",
                default: config.rateLimitRequests,
                when: (a: any) => a.enableRateLimit,
            },
            {
                type: "number",
                name: "rateLimitWindow",
                message: "Rate limit window (seconds):",
                default: config.rateLimitWindow,
                when: (a: any) => a.enableRateLimit,
            },
            {
                type: "input",
                name: "pluginNetwork",
                message: "Docker network name for plugin containers:",
                default: config.pluginNetwork,
            },
            {
                type: "number",
                name: "registryPort",
                message: "Docker registry port:",
                default: config.registryPort,
            },
            {
                type: "input",
                name: "localRegistryUrl",
                message: "Local registry URL (for plugin image storage):",
                default: config.localRegistryUrl,
            },
        ]);

        config.dbPassword = advanced.dbPassword;
        config.redisPassword = advanced.redisPassword;
        config.coreImageTag = advanced.coreImageTag;
        config.postgresVersion = advanced.postgresVersion;
        config.sessionTtl = advanced.sessionTtl;
        config.enableCors = advanced.enableCors;
        config.corsOrigins = advanced.corsOrigins;
        config.enableRateLimit = advanced.enableRateLimit;
        config.rateLimitRequests = advanced.rateLimitRequests;
        config.rateLimitWindow = advanced.rateLimitWindow;
        config.pluginNetwork = advanced.pluginNetwork;
        config.registryPort = advanced.registryPort;
        config.localRegistryUrl = advanced.localRegistryUrl;
    }

    return config;
}

function generateCompose(config: CoreConfig): string {
    const securityVars = `
      SECURITY_COOKIE_SECURE: "false"
      SECURITY_COOKIE_SAMESITE: "lax"
      SECURITY_COOKIE_HTTP_ONLY: "true"
      SECURITY_CORS_ENABLED: "${config.enableCors}"
      SECURITY_CORS_ORIGINS: "${config.corsOrigins}"
      SECURITY_RATE_LIMIT_ENABLED: "${config.enableRateLimit}"
      RATE_LIMIT_AUTH_REQUESTS: "${config.rateLimitRequests}"
      RATE_LIMIT_AUTH_WINDOW: "${config.rateLimitWindow}"
      RATE_LIMIT_API_REQUESTS: "${config.rateLimitRequests}"
      RATE_LIMIT_API_WINDOW: "${config.rateLimitWindow}"
      SECURITY_PASSWORD_MIN_LENGTH: "8"
      SECURITY_SESSION_FIXATION: "true"`;

    return `services:
  nginx:
    image: nginx:alpine
    ports:
      - "${config.corePort}:${config.corePort}"
    volumes:
      - ./nginx.conf:/etc/nginx/conf.d/default.conf:ro
    depends_on:
      - core
  core:
    image: ${config.coreImageTag}
    depends_on:
      postgres:
        condition: service_healthy
      redis:
        condition: service_healthy
    environment:
      DATABASE_URL: postgres://postgres:${config.dbPassword}@postgres:5432/alcedo
      REDIS_URL: redis://:${config.redisPassword || ""}@redis:6379
      CORE_PORT: "8080"
      DEV_MODE: "false"
      RUST_LOG: info
      ADMIN_EMAIL: "${config.adminEmail}"
      ADMIN_PASSWORD: "${config.adminPassword}"
      PLUGIN_NETWORK: ${config.pluginNetwork}
      PLUGINS_DIR: /var/lib/plugin-public
      PLUGINS_DIR: /plugins
      CORE_PUBLIC_URL: "${config.corePublicUrl || `http://localhost:${config.corePort}`}"
      SYSTEM_PLUGINS_URL: "${config.systemPluginsUrl}"
      LOCAL_REGISTRY_URL: "${config.localRegistryUrl}"
      SESSION_TTL_SECONDS: "${config.sessionTtl}"
      REGISTRY_ENCRYPTION_KEY: "${config.registryEncryptionKey}"
      COLLECTION_LOG_FLUSH_MS: "100"${securityVars}
    networks:
      - default
      - ${config.pluginNetwork}
    extra_hosts:
      - "host.docker.internal:host-gateway"
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock
      - plugin-public:/var/lib/plugin-public
      - ./plugins:/plugins
      - ./plugins:/app/plugins

  registry:
    image: registry:2
    ports:
      - "${config.registryPort}:5000"
    volumes:
      - registry-data:/var/lib/registry
    healthcheck:
      test: ["CMD", "wget", "--spider", "http://localhost:5000/"]
      interval: 10s
      timeout: 5s
      retries: 3

  postgres:
    image: postgres:${config.postgresVersion}
    environment:
      POSTGRES_USER: postgres
      POSTGRES_PASSWORD: ${config.dbPassword}
      POSTGRES_DB: alcedo
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U postgres"]
      interval: 5s
      timeout: 5s
      retries: 5
    volumes:
      - postgres-data:/var/lib/postgresql/data

  redis:
    image: redis:7-alpine
    restart: unless-stopped
    command: ${config.redisPassword ? `["redis-server", "--requirepass", "${config.redisPassword}"]` : '["redis-server"]'}
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 5s
      timeout: 3s
      retries: 5
    volumes:
      - redis-data:/data

networks:
  default:
  ${config.pluginNetwork}:
    driver: bridge

volumes:
  postgres-data:
  redis-data:
  registry-data:
  plugin-public:
`;
}

function generateNginxConf(config: CoreConfig): string {
    return `upstream core_backend {
    server core:8080;
}

server {
    listen ${config.corePort};
    server_name _;
    absolute_redirect off;

    location / {
        proxy_pass http://core_backend;
        proxy_set_header Host \$host;
        proxy_set_header X-Real-IP \$remote_addr;
        proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto \$scheme;
        proxy_read_timeout 120s;
        proxy_send_timeout 120s;
    }
}
`;
}

function generateEnv(config: CoreConfig): string {
    return `# Alcedo Core — Environment Configuration
# Generated by \`alcedo init-core\`

# Admin credentials (used for first-run bootstrap)
ADMIN_EMAIL=${config.adminEmail}
ADMIN_PASSWORD=${config.adminPassword}

# Database
POSTGRES_PASSWORD=${config.dbPassword}
DATABASE_URL=postgres://postgres:${config.dbPassword}@postgres:5432/alcedo

# Redis
${config.redisPassword ? `REDIS_PASSWORD=${config.redisPassword}\nREDIS_URL=redis://:${config.redisPassword}@redis:6379` : "REDIS_URL=redis://redis:6379"}

# Core
CORE_PORT=${config.corePort}
CORE_PUBLIC_URL=${config.corePublicUrl || `http://localhost:${config.corePort}`}
SYSTEM_PLUGINS_URL=${config.systemPluginsUrl}
LOCAL_REGISTRY_URL=${config.localRegistryUrl}
SESSION_TTL_SECONDS=${config.sessionTtl}
SECURITY_CORS_ENABLED=${config.enableCors}
SECURITY_CORS_ORIGINS=${config.corsOrigins}
SECURITY_RATE_LIMIT_ENABLED=${config.enableRateLimit}
RATE_LIMIT_AUTH_REQUESTS=${config.rateLimitRequests}
RATE_LIMIT_AUTH_WINDOW=${config.rateLimitWindow}
RATE_LIMIT_API_REQUESTS=${config.rateLimitRequests}
RATE_LIMIT_API_WINDOW=${config.rateLimitWindow}

# Docker Registry
REGISTRY_PORT=${config.registryPort}
REGISTRY_ENCRYPTION_KEY=${config.registryEncryptionKey}
`;
}

function generateK8sManifests(config: CoreConfig): Record<string, string> {
    const namespace = config.pluginNetwork || "alcedo-core";
    const dbName = "plugin_core";
    const dbUrlBase = `postgres://postgres:${config.dbPassword}@postgres:5432/${dbName}`;

    return {
        "00-namespace.yaml": `apiVersion: v1
kind: Namespace
metadata:
  name: ${namespace}
`,

        "02-configmap.yaml": `apiVersion: v1
kind: ConfigMap
metadata:
  name: core-config
  namespace: ${namespace}
data:
  CORE_PORT: "${config.corePort}"
  DATABASE_URL: "${dbUrlBase}"
  REDIS_URL: "redis://redis:6379"
  DEV_MODE: "false"
  RUST_LOG: "info"
  PLUGIN_NETWORK: "${namespace}"
  CORE_PUBLIC_URL: "${config.corePublicUrl || `http://localhost:${config.corePort}`}"
  SYSTEM_PLUGINS_URL: "${config.systemPluginsUrl}"
  LOCAL_REGISTRY_URL: "${config.localRegistryUrl}"
  SESSION_TTL_SECONDS: "${config.sessionTtl}"
  COLLECTION_LOG_FLUSH_MS: "100"
  RATE_LIMIT_AUTH_REQUESTS: "${config.rateLimitRequests}"
  RATE_LIMIT_AUTH_WINDOW: "${config.rateLimitWindow}"
  RATE_LIMIT_API_REQUESTS: "${config.rateLimitRequests}"
  RATE_LIMIT_API_WINDOW: "${config.rateLimitWindow}"
  SECURITY_COOKIE_SECURE: "false"
  SECURITY_COOKIE_SAMESITE: "lax"
  SECURITY_COOKIE_HTTP_ONLY: "true"
  SECURITY_SESSION_FIXATION: "true"
  SECURITY_PASSWORD_MIN_LENGTH: "8"
  PLUGINS_DIR: "/plugins"
  PLUGINS_DIR: "/var/lib/plugin-public"
`,

        "03-secrets.yaml": `apiVersion: v1
kind: Secret
metadata:
  name: core-secrets
  namespace: ${namespace}
type: Opaque
stringData:
  REGISTRY_ENCRYPTION_KEY: "${config.registryEncryptionKey}"
  POSTGRES_PASSWORD: "${config.dbPassword}"
  ADMIN_EMAIL: "${config.adminEmail}"
  ADMIN_PASSWORD: "${config.adminPassword}"
`,

        "04-postgres.yaml": `apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: postgres
  namespace: ${namespace}
spec:
  serviceName: postgres
  replicas: 1
  selector:
    matchLabels:
      app: postgres
  volumeClaimTemplates:
    - metadata:
        name: postgres-data
      spec:
        accessModes: ["ReadWriteOnce"]
        resources:
          requests:
            storage: 10Gi
        storageClassName: local-path
  template:
    metadata:
      labels:
        app: postgres
    spec:
      containers:
        - name: postgres
          image: postgres:${config.postgresVersion}
          env:
            - name: POSTGRES_USER
              value: postgres
            - name: POSTGRES_PASSWORD
              valueFrom:
                secretKeyRef:
                  name: core-secrets
                  key: POSTGRES_PASSWORD
            - name: POSTGRES_DB
              value: ${dbName}
          ports:
            - containerPort: 5432
          volumeMounts:
            - name: postgres-data
              mountPath: /var/lib/postgresql/data
              subPath: postgres
          readinessProbe:
            exec:
              command: ["pg_isready", "-U", "postgres"]
            initialDelaySeconds: 5
            periodSeconds: 5
---
apiVersion: v1
kind: Service
metadata:
  name: postgres
  namespace: ${namespace}
spec:
  selector:
    app: postgres
  ports:
    - port: 5432
      targetPort: 5432
`,

        "05-redis.yaml": `apiVersion: apps/v1
kind: Deployment
metadata:
  name: redis
  namespace: ${namespace}
spec:
  replicas: 1
  selector:
    matchLabels:
      app: redis
  template:
    metadata:
      labels:
        app: redis
    spec:
      containers:
        - name: redis
          image: redis:7-alpine
          ports:
            - containerPort: 6379
          readinessProbe:
            exec:
              command: ["redis-cli", "ping"]
            initialDelaySeconds: 3
            periodSeconds: 5
---
apiVersion: v1
kind: Service
metadata:
  name: redis
  namespace: ${namespace}
spec:
  selector:
    app: redis
  ports:
    - port: 6379
      targetPort: 6379
`,

        "06-core-deployment.yaml": `apiVersion: apps/v1
kind: Deployment
metadata:
  name: alcedo-core
  namespace: ${namespace}
  labels:
    app.kubernetes.io/name: alcedo-core
    app.kubernetes.io/managed-by: alcedo-core
spec:
  replicas: 1
  selector:
    matchLabels:
      app: alcedo-core
  template:
    metadata:
      labels:
        app: alcedo-core
        app.kubernetes.io/name: alcedo-core
        app.kubernetes.io/managed-by: alcedo-core
    spec:
      serviceAccountName: alcedo-core
      securityContext:
        fsGroup: 1001
      initContainers:
        - name: seed-plugins
          image: ${config.coreImageTag}
          imagePullPolicy: Always
          command: ["sh", "-c", "cp -r /plugins/* /data/ 2>/dev/null; chmod -R 755 /data/"]
          securityContext:
            runAsUser: 0
          volumeMounts:
            - name: plugins
              mountPath: /data
            - name: plugin-data
              mountPath: /var/lib/plugin-public
      containers:
        - name: core
          image: ${config.coreImageTag}
          imagePullPolicy: Always
          ports:
            - containerPort: 8080
              name: http
          envFrom:
            - configMapRef:
                name: core-config
            - secretRef:
                name: core-secrets
          env:
            - name: POD_NAMESPACE
              valueFrom:
                fieldRef:
                  fieldPath: metadata.namespace
            - name: POD_NAME
              valueFrom:
                fieldRef:
                  fieldPath: metadata.name
          volumeMounts:
            - name: plugins
              mountPath: /plugins
            - name: plugin-data
              mountPath: /var/lib/plugin-public
          readinessProbe:
            httpGet:
              path: /health
              port: 8080
            initialDelaySeconds: 5
            periodSeconds: 10
          livenessProbe:
            httpGet:
              path: /health
              port: 8080
            initialDelaySeconds: 15
            periodSeconds: 20
          resources:
            requests:
              cpu: 100m
              memory: 256Mi
            limits:
              cpu: 500m
              memory: 512Mi
      volumes:
        - name: plugins
          emptyDir: {}
        - name: plugin-data
          persistentVolumeClaim:
            claimName: plugin-data
`,

        "07-core-service.yaml": `apiVersion: v1
kind: Service
metadata:
  name: alcedo-core
  namespace: ${namespace}
  labels:
    app.kubernetes.io/name: alcedo-core
spec:
  selector:
    app: alcedo-core
  ports:
    - port: 8080
      targetPort: 8080
      name: http
  type: ClusterIP
`,

        "08-plugin-data-pvc.yaml": `apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: plugin-data
  namespace: ${namespace}
  labels:
    app.kubernetes.io/name: alcedo-core
    app.kubernetes.io/managed-by: alcedo-core
spec:
  accessModes:
    - ReadWriteOnce
  resources:
    requests:
      storage: 1Gi
`,
    };
}

async function writeFiles(
    targetDir: string,
    config: CoreConfig,
): Promise<void> {
    const spinner = createSpinner("Generating Alcedo Core configuration");

    try {
        fs.mkdirSync(targetDir, { recursive: true });

        if (config.platform === "k8s") {
            const k8sDir = path.join(targetDir, "k8s");
            fs.mkdirSync(k8sDir, { recursive: true });

            const manifests = generateK8sManifests(config);
            for (const [filename, content] of Object.entries(manifests)) {
                fs.writeFileSync(path.join(k8sDir, filename), content, "utf-8");
            }

            // Create plugins directory
            const pluginsDir = path.join(targetDir, "plugins");
            fs.mkdirSync(pluginsDir, { recursive: true });

            spinner.succeed();
            success(`Alcedo Core K8s configuration generated: ${targetDir}`);
            info("");
            info("To deploy Alcedo Core on Kubernetes:");
            info(`  cd ${path.relative(process.cwd(), targetDir)}`);
            info("  kubectl apply -f k8s/");
            info("");
            info("The admin UI will be available after port-forwarding:");
            info(
                "  kubectl port-forward -n alcedo-core svc/alcedo-core 8080:8080",
            );
            info(
                `  ${config.corePublicUrl || `http://localhost:${config.corePort}`}/admin`,
            );
            info(`  Email: ${config.adminEmail}`);
            info("");
            info("Static plugins directory:");
            info(`  ${path.join(targetDir, "plugins")}/`);
            info("  Place static plugin folders (with manifest.json) here.");
        } else {
            // docker-compose.yml
            const compose = generateCompose(config);
            fs.writeFileSync(
                path.join(targetDir, "docker-compose.yml"),
                compose,
                "utf-8",
            );

            // .env
            const env = generateEnv(config);
            fs.writeFileSync(path.join(targetDir, ".env"), env, "utf-8");

            const nginx = generateNginxConf(config);
            fs.writeFileSync(
                path.join(targetDir, "nginx.conf"),
                nginx,
                "utf-8",
            );

            // Create plugins directory for static system plugins (e.g. admin UI)
            const pluginsDir = path.join(targetDir, "plugins");
            fs.mkdirSync(pluginsDir, { recursive: true });
            // Create a README so users know they can place static plugins here
            fs.writeFileSync(
                path.join(pluginsDir, "README.md"),
                "# Static Plugins\n\nPlace static plugin folders here (each with a manifest.json).\n" +
                    "The core will load them automatically on startup.\n",
                "utf-8",
            );

            spinner.succeed();
            success(`Alcedo Core configuration generated: ${targetDir}`);
            info("");
            info("To start Alcedo Core:");
            info(`  cd ${path.relative(process.cwd(), targetDir)}`);
            info("  docker compose up -d");
            info("");
            info("The admin UI will be available at:");
            info(
                `  ${config.corePublicUrl || `http://localhost:${config.corePort}`}/admin`,
            );
            info(`  Email: ${config.adminEmail}`);
            info("");
            info("Local Docker Registry:");
            info(`  http://localhost:${config.registryPort}`);
            info("");
            info("Static plugins directory:");
            info(`  ${path.join(targetDir, "plugins")}/`);
            info("  Place static plugin folders (with manifest.json) here.");
            info("System plugins will be auto-deployed from:");
            info(`  ${config.systemPluginsUrl}`);
            info("");
            info("To deploy a plugin:");
            info(
                `  curl -X POST http://localhost:${config.corePort}/api/plugins/deploy \\`,
            );
            info('    -H "Content-Type: application/json" \\');
            info(
                '    -d \'{"slug": "my-plugin", "image": "localhost:5000/my-plugin:latest"\'}',
            );
        }
    } catch (err: any) {
        spinner.fail();
        throw err;
    }
}
