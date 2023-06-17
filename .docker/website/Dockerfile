FROM node

ARG $WEBSITE_PORT

WORKDIR /home/node/app
COPY ./website/package*.json .

RUN npm install
COPY ./website .

EXPOSE $WEBSITE_PORT
CMD ["npm", "start"]