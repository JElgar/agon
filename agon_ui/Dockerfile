FROM node:24-alpine AS build

# Set up working directory
WORKDIR /app

# Copy package.json and package-lock.json (or yarn.lock) first for better cache performance
COPY package*.json ./

# Install dependencies
RUN npm install

# Add the rest of the application files
COPY . /app/
 
# Build the React app for production 
RUN npm run build

FROM nginx:alpine

# Copy the built React files from the build stage to Nginx
COPY --from=build /app/dist /usr/share/nginx/html

COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

# Expose the port
EXPOSE 80

ENTRYPOINT ["/entrypoint.sh"]
