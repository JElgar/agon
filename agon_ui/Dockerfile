FROM node:24-alpine AS build
# Set up environment as production 
ENV NODE_ENV=production
 
# Set up working directory 
WORKDIR /app
 
# Copy package.json and package-lock.json (or yarn.lock) first for better cache performance 
COPY package*.json ./


# Install dependencies 
RUN npm install --production
 
# Add the rest of the application files 
COPY . /app/
 
# Build the React app for production 
RUN npm run build

FROM nginx:alpine
 
# Copy the built React files from the build stage to Nginx 
COPY --from=build /app/dist /usr/share/nginx/html
 
# Expose the port 
EXPOSE 80
 
# Start Nginx to serve the React app 
CMD ["nginx", "-g", "daemon off;"]
